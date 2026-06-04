# rustdl Python Bindings Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship `rustdl` on PyPI — pip-installable Python binding for rustdl's classifier and consistency checker via PyO3 + maturin, with multi-platform wheels through cibuildwheel.

**Architecture:** New `crates/owl-dl-py/` crate in the rustdl monorepo. PyO3 thin wrapper around `owl-dl-reasoner`'s public API. ABI3-py310 wheel (one wheel for Python 3.10–3.13). CI workflow builds wheels for 5 platforms via cibuildwheel. PyPI publish via trusted-publisher (no token in CI).

**Tech Stack:** PyO3 0.22 (extension-module + abi3-py310 features), maturin 1.7+, cibuildwheel 2.x, pytest. Reuses `owl-dl-reasoner` (path dep + crates.io version pin).

**Spec:** [docs/superpowers/specs/2026-06-04-python-bindings-design.md](../specs/2026-06-04-python-bindings-design.md)

---

## File Structure

| File | Action | Responsibility |
|---|---|---|
| `Cargo.toml` (root) | Modify | Add `crates/owl-dl-py` to `[workspace] members`; exclude from `default-members` so `cargo build --workspace` doesn't try to build cdylib for everyone |
| `crates/owl-dl-py/Cargo.toml` | Create | PyO3 dependencies, `crate-type = ["cdylib"]`, `publish = false` (PyPI publish is via maturin, not cargo) |
| `crates/owl-dl-py/pyproject.toml` | Create | maturin build backend, package metadata, dependency declarations |
| `crates/owl-dl-py/src/lib.rs` | Create | `#[pymodule] fn rustdl(_py, m)` — registers all bindings |
| `crates/owl-dl-py/src/errors.rs` | Create | Python exception types + `ReasonError`/`HornedError` → Python mapping |
| `crates/owl-dl-py/src/load.rs` | Create | File-path + bytes loaders with format detection |
| `crates/owl-dl-py/src/classify.rs` | Create | `classify`, `classify_bytes`, `Classification` PyO3 class |
| `crates/owl-dl-py/src/queries.rs` | Create | `is_consistent`, `is_class_satisfiable`, `is_subclass_of`, `is_instance_of`, `instances_of`, `realize` |
| `crates/owl-dl-py/src/materialize.rs` | Create | `materialize_inferred_subclass_axioms`, `materialize_inferred_class_assertions` |
| `crates/owl-dl-py/python/rustdl/__init__.py` | Create | Re-exports from native ext + Python-side `subclasses_of`/`superclasses_of` helpers |
| `crates/owl-dl-py/python/rustdl/_native.pyi` | Create | Type stubs for the native module (IDE autocomplete + mypy) |
| `crates/owl-dl-py/tests/python/test_*.py` | Create | pytest integration tests |
| `crates/owl-dl-py/tests/python/conftest.py` | Create | Fixture-path resolution helper |
| `.github/workflows/python-ci.yml` | Create | maturin develop + pytest on every PR/push |
| `.github/workflows/release-python.yml` | Create | cibuildwheel matrix + maturin publish on `v*.*.*` tag |
| `README.md` | Modify | Add a one-paragraph "Python" section with install + minimal example |
| `CHANGELOG.md` | Modify | Add entry for Python bindings when 0.1.1 (or 0.2.0) tags |

---

## Pre-flight

### Task 0: Baseline + branch decision

**Files:** none

- [ ] **Step 1: Confirm clean tree + on main**

```sh
cd /data/dumontier/rustdl
git status --short
git rev-parse --abbrev-ref HEAD
git log --oneline -3
```

Expected: branch `main`. Top commit is `3e2edf1 spec: rustdl Python bindings …`. Working tree may show `.claude/settings.json` (modified — do not stage) and a couple of untracked items (`docs/flamegraphs/...svg`, `scripts/phase2b-build-fixtures.sh`).

- [ ] **Step 2: Verify baseline build + test passes**

```sh
export PATH=$HOME/.cargo/bin:/home/dumontier/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH
cargo build --workspace --release 2>&1 | tail -3
cargo test -p owl-dl-reasoner --release --lib 2>&1 | grep "test result"
```

Expected: build succeeds; `test result: FAILED. 83 passed; 6 failed; …`. The 6 pre-existing failures are accepted baseline.

- [ ] **Step 3: Check that maturin + cibuildwheel aren't already installed at unexpected versions**

```sh
pip show maturin 2>/dev/null | grep Version
pip show cibuildwheel 2>/dev/null | grep Version
which python3
python3 --version
```

Expected: `python3 --version` ≥ 3.10. maturin / cibuildwheel may be absent — that's fine, the build will use whatever cibuildwheel installs in CI; for local dev we'll install maturin in T2.

- [ ] **Step 4: Branch decision**

Per session convention (documentation + release work on `main` — the most recent v0.1.0 release was done this way), default to main. If the implementer prefers a feature branch for review safety, `git checkout -b feat/python-bindings` is acceptable. Tasks below assume the current branch.

---

## Phase 1: Scaffold crate

### Task 1: Create the `owl-dl-py` crate skeleton

**Files:**
- Create: `crates/owl-dl-py/Cargo.toml`
- Create: `crates/owl-dl-py/src/lib.rs`
- Create: `crates/owl-dl-py/pyproject.toml`
- Create: `crates/owl-dl-py/python/rustdl/__init__.py`
- Create: `crates/owl-dl-py/.gitignore`
- Modify: `Cargo.toml` (root) — add `crates/owl-dl-py` to `[workspace] members` (NOT to `default-members`)

- [ ] **Step 1: Create `crates/owl-dl-py/Cargo.toml`**

```toml
[package]
name = "owl-dl-py"
description = "Python bindings for the rustdl OWL DL reasoner (via PyO3 + maturin)"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true
authors.workspace = true
repository.workspace = true
readme = "README.md"
keywords = ["owl", "ontology", "reasoner", "python", "pyo3"]
categories = ["science", "api-bindings"]
publish = false  # published via maturin → PyPI, not via cargo → crates.io

[lib]
name = "rustdl"           # Python import name: `import rustdl`
crate-type = ["cdylib"]   # Python C extension

[dependencies]
pyo3 = { version = "0.22", features = ["abi3-py310", "extension-module"] }
owl-dl-reasoner.workspace = true
owl-dl-core.workspace = true
horned-owl.workspace = true
thiserror.workspace = true

[lints]
workspace = true
```

- [ ] **Step 2: Create `crates/owl-dl-py/src/lib.rs` with an empty pymodule**

```rust
//! Python bindings for the rustdl OWL DL reasoner.
//!
//! Built with PyO3 + maturin. Distributed on PyPI as `rustdl`;
//! imported in Python as `import rustdl`. See the spec at
//! `docs/superpowers/specs/2026-06-04-python-bindings-design.md`.

use pyo3::prelude::*;

/// The Python module that PyO3 exposes. Adding new top-level functions
/// or classes is a one-line `m.add_function` / `m.add_class` call here
/// — implementations live in sibling modules.
#[pymodule]
fn rustdl(_py: Python<'_>, _m: &Bound<'_, PyModule>) -> PyResult<()> {
    Ok(())
}
```

- [ ] **Step 3: Create `crates/owl-dl-py/pyproject.toml`**

```toml
[build-system]
requires = ["maturin>=1.7,<2"]
build-backend = "maturin"

[project]
name = "rustdl"
description = "Sound, performant OWL 2 DL (SROIQ) reasoner in Rust — Python bindings"
readme = "README.md"
license = { text = "Apache-2.0 OR MIT" }
authors = [{ name = "rustdl contributors" }]
requires-python = ">=3.10"
classifiers = [
    "Development Status :: 4 - Beta",
    "Intended Audience :: Science/Research",
    "License :: OSI Approved :: Apache Software License",
    "License :: OSI Approved :: MIT License",
    "Programming Language :: Python :: 3",
    "Programming Language :: Python :: 3.10",
    "Programming Language :: Python :: 3.11",
    "Programming Language :: Python :: 3.12",
    "Programming Language :: Python :: 3.13",
    "Programming Language :: Rust",
    "Topic :: Scientific/Engineering",
]
keywords = ["owl", "ontology", "reasoner", "description-logic", "semantic-web"]
dynamic = ["version"]   # version comes from Cargo.toml via maturin

[project.urls]
Homepage = "https://github.com/MaastrichtU-IDS/rustdl"
Repository = "https://github.com/MaastrichtU-IDS/rustdl"
Documentation = "https://github.com/MaastrichtU-IDS/rustdl/blob/main/README.md"
Changelog = "https://github.com/MaastrichtU-IDS/rustdl/blob/main/CHANGELOG.md"

[tool.maturin]
python-source = "python"     # pure-Python sources live in python/
module-name = "rustdl._native"  # the compiled extension lands at rustdl._native
features = ["pyo3/extension-module"]
strip = true
```

> **Why `module-name = "rustdl._native"`:** maturin best practice for ABI3 wheels with a Python wrapper layer. The native extension is `rustdl._native`, and `python/rustdl/__init__.py` re-exports + augments it.

- [ ] **Step 4: Create the Python wrapper `crates/owl-dl-py/python/rustdl/__init__.py`**

```python
"""
rustdl — sound, performant OWL 2 DL (SROIQ) reasoner.

Python bindings for the rustdl Rust crate. Install via
`pip install rustdl`; import as `import rustdl`. See
https://github.com/MaastrichtU-IDS/rustdl for the full project.
"""

# Re-export the native extension's public surface.
from rustdl._native import (
    __version__ as __version__,
)

__all__ = ["__version__"]
```

- [ ] **Step 5: Create `crates/owl-dl-py/.gitignore`**

```
target/
*.so
*.pyd
*.dylib
__pycache__/
*.egg-info/
dist/
build/
.venv/
```

- [ ] **Step 6: Add to root `Cargo.toml` workspace**

Find the `[workspace] members = [ ... ]` block. Add `"crates/owl-dl-py"` to `members` but NOT to `default-members` (so `cargo build --workspace` from the default-members list doesn't pull in the cdylib, which has different build characteristics than the lib crates).

Existing:
```toml
[workspace]
resolver = "2"
members = [
    "crates/owl-dl-core",
    "crates/owl-dl-saturation",
    "crates/owl-dl-tableau",
    "crates/owl-dl-datatypes",
    "crates/owl-dl-reasoner",
    "crates/owl-dl-cli",
    "crates/owl-dl-bench",
    "xtask",
]
default-members = [
    "crates/owl-dl-core",
    "crates/owl-dl-saturation",
    "crates/owl-dl-tableau",
    "crates/owl-dl-datatypes",
    "crates/owl-dl-reasoner",
    "crates/owl-dl-cli",
    "crates/owl-dl-bench",
]
```

Modify to:
```toml
[workspace]
resolver = "2"
members = [
    "crates/owl-dl-core",
    "crates/owl-dl-saturation",
    "crates/owl-dl-tableau",
    "crates/owl-dl-datatypes",
    "crates/owl-dl-reasoner",
    "crates/owl-dl-cli",
    "crates/owl-dl-bench",
    "crates/owl-dl-py",
    "xtask",
]
default-members = [
    "crates/owl-dl-core",
    "crates/owl-dl-saturation",
    "crates/owl-dl-tableau",
    "crates/owl-dl-datatypes",
    "crates/owl-dl-reasoner",
    "crates/owl-dl-cli",
    "crates/owl-dl-bench",
]
```

- [ ] **Step 7: Verify scaffolding builds (without maturin yet)**

```sh
export PATH=$HOME/.cargo/bin:/home/dumontier/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH
cargo build -p owl-dl-py --release 2>&1 | tail -5
```

Expected: build succeeds. The cdylib is at `target/release/librustdl.so` (or `.dylib` / `.dll`).

If clippy fires on the empty pymodule:
```sh
cargo clippy -p owl-dl-py --all-targets -- -D warnings 2>&1 | tail -5
```
Expected: clean. PyO3's macros sometimes need `#[allow]` annotations — if so, add minimal ones at the function signature, not crate-wide.

- [ ] **Step 8: Commit**

```sh
git add crates/owl-dl-py Cargo.toml
git restore --staged .claude/settings.json 2>/dev/null || true
git status --short
git commit -m "$(cat <<'EOF'
feat(py): T1 — owl-dl-py crate skeleton (PyO3 + maturin)

New crate at crates/owl-dl-py/. Empty #[pymodule] fn rustdl plus
pyproject.toml declaring maturin as the build backend, abi3-py310
mode (one wheel per platform across Python 3.10-3.13). Wired into
workspace.members but kept out of default-members so cargo
build --workspace doesn't pull in the cdylib build.

Spec: docs/superpowers/specs/2026-06-04-python-bindings-design.md
EOF
)"
```

---

### Task 2: Install maturin locally + verify wheel builds

**Files:** none (verification + commit `.python-version` if desired)

- [ ] **Step 1: Install maturin**

```sh
pip install --user 'maturin>=1.7,<2'
maturin --version
```

Expected: maturin 1.7.x or later printed.

- [ ] **Step 2: Create a Python venv for testing the bindings**

```sh
python3 -m venv /tmp/rustdl-py-venv
source /tmp/rustdl-py-venv/bin/activate
pip install pytest
```

- [ ] **Step 3: Develop-install the bindings**

```sh
cd crates/owl-dl-py
maturin develop --release 2>&1 | tail -10
cd ../..
```

Expected: `Built wheel … installed package rustdl`. Now `python -c "import rustdl"` works.

- [ ] **Step 4: Smoke test**

```sh
python -c "import rustdl; print(rustdl.__version__)"
```

Expected: prints `0.1.0` (the workspace version inherited via `dynamic = ["version"]`).

If `__version__` isn't auto-injected by maturin, add the following near the top of `src/lib.rs`:

```rust
#[pymodule]
fn rustdl(_py: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    Ok(())
}
```

Then redo `maturin develop --release` and the smoke test.

- [ ] **Step 5: Run the (empty) test suite**

```sh
mkdir -p crates/owl-dl-py/tests/python
echo 'def test_smoke():
    import rustdl
    assert rustdl.__version__' > crates/owl-dl-py/tests/python/test_smoke.py
pytest crates/owl-dl-py/tests/python/ -v 2>&1 | tail -5
```

Expected: 1 test passed.

- [ ] **Step 6: Commit**

```sh
git add crates/owl-dl-py/src/lib.rs crates/owl-dl-py/tests/python/test_smoke.py
git restore --staged .claude/settings.json 2>/dev/null || true
git commit -m "$(cat <<'EOF'
test(py): T2 — smoke test confirms maturin develop + import works

`python -c "import rustdl; print(rustdl.__version__)"` returns
"0.1.0". One pytest test guards regression of the import path.
The version is injected via CARGO_PKG_VERSION so workspace bumps
propagate without touching pyproject.toml.

Spec: docs/superpowers/specs/2026-06-04-python-bindings-design.md
EOF
)"
```

---

## Phase 2: Errors + loaders

### Task 3: Exception hierarchy + ReasonError mapping

**Files:**
- Create: `crates/owl-dl-py/src/errors.rs`
- Modify: `crates/owl-dl-py/src/lib.rs` (add `mod errors;` + register exception types)

- [ ] **Step 1: Create `crates/owl-dl-py/src/errors.rs`**

```rust
//! Python exception types + mapping from Rust `ReasonError` /
//! horned-owl parse errors into Python exceptions.
//!
//! Hierarchy:
//!
//! ```text
//! Exception
//! └── RustdlError                 (base — catches everything from rustdl)
//!     ├── ParseError              (horned-owl parse failure)
//!     ├── UnsupportedAxiomError   (HasKey, length-3+ chains, data ranges, ...)
//!     └── UnknownClassError       (IRI not declared as a class)
//! ```

use owl_dl_core::convert::ConversionError;
use owl_dl_reasoner::ReasonError;
use pyo3::create_exception;
use pyo3::exceptions::PyException;
use pyo3::prelude::*;

create_exception!(rustdl, RustdlError, PyException, "Base exception for all rustdl errors.");
create_exception!(rustdl, ParseError, RustdlError, "OWL parser failure.");
create_exception!(rustdl, UnsupportedAxiomError, RustdlError, "Axiom or class expression rustdl can't represent.");
create_exception!(rustdl, UnknownClassError, RustdlError, "Class IRI not declared in the ontology.");

/// Register all rustdl exception types on the module so Python
/// callers can `except rustdl.RustdlError:` etc.
pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("RustdlError", m.py().get_type_bound::<RustdlError>())?;
    m.add("ParseError", m.py().get_type_bound::<ParseError>())?;
    m.add("UnsupportedAxiomError", m.py().get_type_bound::<UnsupportedAxiomError>())?;
    m.add("UnknownClassError", m.py().get_type_bound::<UnknownClassError>())?;
    Ok(())
}

/// Map a `ReasonError` into the appropriate Python exception.
pub(crate) fn reason_error_to_py(err: ReasonError) -> PyErr {
    match err {
        ReasonError::Conversion(conv) => match conv {
            ConversionError::UnsupportedAxiom { kind } => {
                UnsupportedAxiomError::new_err(format!("unsupported axiom: {kind}"))
            }
            ConversionError::UnsupportedConcept { kind } => {
                UnsupportedAxiomError::new_err(format!("unsupported concept: {kind}"))
            }
            ConversionError::AnonymousIndividual => {
                UnsupportedAxiomError::new_err("anonymous individuals are not supported")
            }
            ConversionError::UnsupportedDataRange => {
                UnsupportedAxiomError::new_err("data ranges and data properties are not supported")
            }
        },
        ReasonError::UnknownClass(iri) => UnknownClassError::new_err(iri),
        ReasonError::NoVerdict => {
            RustdlError::new_err("internal: tableau returned NoVerdict (please file a bug)")
        }
        ReasonError::RoleChainUnsupported => {
            UnsupportedAxiomError::new_err("role chain longer than length 2 (unsupported)")
        }
    }
}
```

> **Note:** `ConversionError` and `ReasonError` are imported from their actual crates. Verify exact paths: `owl_dl_core::convert::ConversionError` and `owl_dl_reasoner::ReasonError`. If the implementer hits a "module not found" error, the right move is to grep `crates/owl-dl-core/src/lib.rs` for the public re-export of `convert`; the crate may re-export it as `owl_dl_core::ConversionError`.

- [ ] **Step 2: Wire into `src/lib.rs`**

Replace the current `src/lib.rs` body with:

```rust
//! Python bindings for the rustdl OWL DL reasoner.
//!
//! Built with PyO3 + maturin. Distributed on PyPI as `rustdl`;
//! imported in Python as `import rustdl`. See the spec at
//! `docs/superpowers/specs/2026-06-04-python-bindings-design.md`.

use pyo3::prelude::*;

mod errors;

#[pymodule]
fn rustdl(_py: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    errors::register(m)?;
    Ok(())
}
```

- [ ] **Step 3: Rebuild + verify exceptions are exposed in Python**

```sh
cd crates/owl-dl-py
maturin develop --release 2>&1 | tail -3
cd ../..
source /tmp/rustdl-py-venv/bin/activate
python -c "import rustdl; print(rustdl.RustdlError.__mro__)"
```

Expected: prints `(<class 'rustdl.RustdlError'>, <class 'Exception'>, …)` or similar; `RustdlError` is on the module.

- [ ] **Step 4: Smoke-test the exception hierarchy from Python**

Add to `crates/owl-dl-py/tests/python/test_smoke.py`:

```python
def test_exception_hierarchy():
    import rustdl
    assert issubclass(rustdl.ParseError, rustdl.RustdlError)
    assert issubclass(rustdl.UnsupportedAxiomError, rustdl.RustdlError)
    assert issubclass(rustdl.UnknownClassError, rustdl.RustdlError)
    assert issubclass(rustdl.RustdlError, Exception)
```

Run:
```sh
pytest crates/owl-dl-py/tests/python/test_smoke.py -v 2>&1 | tail -5
```
Expected: both tests pass.

- [ ] **Step 5: Commit**

```sh
git add crates/owl-dl-py/src/errors.rs crates/owl-dl-py/src/lib.rs crates/owl-dl-py/tests/python/test_smoke.py
git restore --staged .claude/settings.json 2>/dev/null || true
git commit -m "$(cat <<'EOF'
feat(py): T3 — exception hierarchy + ReasonError mapping

Four Python exception types — RustdlError (base) and three children
(ParseError, UnsupportedAxiomError, UnknownClassError). Conversion
helper maps every ReasonError variant to one. Smoke test confirms
the hierarchy at the Python level.

Spec: docs/superpowers/specs/2026-06-04-python-bindings-design.md
EOF
)"
```

---

### Task 4: Load helpers — file path + bytes input with format detection

**Files:**
- Create: `crates/owl-dl-py/src/load.rs`
- Modify: `crates/owl-dl-py/src/lib.rs` (`mod load;`)

- [ ] **Step 1: Create `crates/owl-dl-py/src/load.rs`**

```rust
//! Parse OWL ontologies from a file path or bytes, mapping
//! horned-owl parse errors to `ParseError`.

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::io::owx::reader::read as read_owx;
use horned_owl::io::rdf::reader::read as read_rdf;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use pyo3::PyResult;
use std::io::Cursor;
use std::path::Path;

use crate::errors::ParseError;

/// Parse an ontology from a file path. Format is auto-detected from
/// the file extension (`.ofn` | `.owx` | `.rdf` | `.owl`).
pub(crate) fn load_path(path: &str) -> PyResult<SetOntology<RcStr>> {
    let src = std::fs::read_to_string(path)
        .map_err(|e| ParseError::new_err(format!("read {path}: {e}")))?;
    let ext = Path::new(path)
        .extension()
        .and_then(|s| s.to_str())
        .map(str::to_ascii_lowercase);
    let format = match ext.as_deref() {
        Some("ofn") => "ofn",
        Some("owx") => "owx",
        Some("rdf" | "owl") => "rdf-xml",
        Some(other) => {
            return Err(ParseError::new_err(format!(
                "unknown extension `.{other}` — pass format= explicitly via classify_bytes() or rename file to .ofn / .owx / .rdf"
            )));
        }
        None => {
            return Err(ParseError::new_err(format!(
                "no extension on `{path}` — pass format= explicitly via classify_bytes()"
            )));
        }
    };
    parse_with_format(&src, format)
}

/// Parse from bytes with an explicit format string.
pub(crate) fn load_bytes(data: &[u8], format: &str) -> PyResult<SetOntology<RcStr>> {
    let src = std::str::from_utf8(data)
        .map_err(|e| ParseError::new_err(format!("ontology bytes are not valid UTF-8: {e}")))?;
    parse_with_format(src, format)
}

fn parse_with_format(src: &str, format: &str) -> PyResult<SetOntology<RcStr>> {
    let mut reader = Cursor::new(src);
    let cfg = ParserConfiguration::default();
    let result = match format {
        "ofn" => read_ofn(&mut reader, cfg).map(|(o, _)| o),
        "owx" => read_owx(&mut reader, cfg).map(|(o, _)| o),
        "rdf-xml" | "rdf" => read_rdf(&mut reader, cfg).map(|(o, _)| o),
        other => {
            return Err(ParseError::new_err(format!(
                "unknown format `{other}` — expected one of: ofn, owx, rdf-xml"
            )));
        }
    };
    result.map_err(|e| ParseError::new_err(format!("parse {format}: {e}")))
}
```

> **Note:** `horned_owl::io::rdf::reader::read` and `horned_owl::io::owx::reader::read` — verify these paths exist at horned-owl 1.4. If a path is wrong, grep `~/.cargo/registry/src/.../horned-owl-1.4.*/src/io/` to find the actual module. The signature may also vary (`read` may take different argument shapes). Match whatever's actually there.

- [ ] **Step 2: Wire into `src/lib.rs`**

Replace the `mod` declarations block:

```rust
mod errors;
mod load;
```

- [ ] **Step 3: Build to confirm horned-owl API matches**

```sh
cargo build -p owl-dl-py --release 2>&1 | tail -10
```

If compile errors on `read_owx` / `read_rdf` paths: grep the actual readers:
```sh
ls ~/.cargo/registry/src/index.crates.io-*/horned-owl-1.4.*/src/io/
```
Update the `use` statements accordingly.

- [ ] **Step 4: Commit**

```sh
git add crates/owl-dl-py/src/load.rs crates/owl-dl-py/src/lib.rs
git restore --staged .claude/settings.json 2>/dev/null || true
git commit -m "$(cat <<'EOF'
feat(py): T4 — load helpers for file path + bytes with format auto-detect

`load_path("ontology.ofn")` auto-detects from extension (.ofn / .owx
/ .rdf / .owl). `load_bytes(data, "ofn")` uses explicit format.
Parse failures map to ParseError. Reuses horned-owl's three readers.

Spec: docs/superpowers/specs/2026-06-04-python-bindings-design.md
EOF
)"
```

---

## Phase 3: Core API — classify + Classification

### Task 5: `classify` + `classify_bytes` + `Classification` PyO3 class

**Files:**
- Create: `crates/owl-dl-py/src/classify.rs`
- Modify: `crates/owl-dl-py/src/lib.rs` (`mod classify;` + register)
- Test: `crates/owl-dl-py/tests/python/test_classify.py`
- Create: `crates/owl-dl-py/tests/python/conftest.py`

- [ ] **Step 1: Create `crates/owl-dl-py/tests/python/conftest.py`**

```python
"""Shared pytest fixtures for the rustdl Python bindings tests."""
import pathlib
import pytest

REPO_ROOT = pathlib.Path(__file__).resolve().parents[4]
FIXTURE_DIR = REPO_ROOT / "crates" / "owl-dl-reasoner" / "tests" / "fixtures"

@pytest.fixture
def fixtures_dir():
    """Resolve to crates/owl-dl-reasoner/tests/fixtures/ — reuses the Rust-side test inputs."""
    return FIXTURE_DIR
```

- [ ] **Step 2: Write the failing test first (TDD)**

`crates/owl-dl-py/tests/python/test_classify.py`:

```python
import rustdl
import pytest


def test_classify_returns_classification(fixtures_dir):
    fixture = fixtures_dir / "datatype" / "datatype_definition.ofn"
    result = rustdl.classify(str(fixture))
    assert isinstance(result, rustdl.Classification)
    assert isinstance(result.classes, list)
    assert len(result.classes) > 0


def test_classification_is_subclass(fixtures_dir):
    fixture = fixtures_dir / "datatype" / "datatype_definition.ofn"
    result = rustdl.classify(str(fixture))
    # In this fixture: Adult ⊑ Person via direct SubClassOf axiom
    assert result.is_subclass("http://t/Adult", "http://t/Person")


def test_classify_bytes_ofn(fixtures_dir):
    fixture = fixtures_dir / "datatype" / "datatype_definition.ofn"
    data = fixture.read_bytes()
    result = rustdl.classify_bytes(data, format="ofn")
    assert "http://t/Adult" in result.classes


def test_classify_unknown_extension_raises(tmp_path):
    bad = tmp_path / "ontology.xyz"
    bad.write_text("Ontology()")
    with pytest.raises(rustdl.ParseError):
        rustdl.classify(str(bad))
```

Run:
```sh
pytest crates/owl-dl-py/tests/python/test_classify.py -v 2>&1 | tail -8
```

Expected: all 4 tests fail with `AttributeError: module 'rustdl' has no attribute 'classify'` (or similar). Good — we have failing tests.

- [ ] **Step 3: Create `crates/owl-dl-py/src/classify.rs`**

```rust
//! `classify` / `classify_bytes` top-level functions + the
//! `Classification` PyO3 class that wraps `owl_dl_reasoner::Classification`.

use owl_dl_reasoner::{Classification as RsClassification, classify as rs_classify};
use pyo3::prelude::*;
use pyo3::types::PyList;

use crate::errors::reason_error_to_py;
use crate::load;

/// Wraps the Rust-side `Classification` for Python consumption.
#[pyclass(name = "Classification", module = "rustdl")]
pub(crate) struct PyClassification {
    inner: RsClassification,
}

#[pymethods]
impl PyClassification {
    /// All declared class IRIs in the ontology, insertion order.
    #[getter]
    fn classes(&self) -> Vec<String> {
        self.inner.classes().to_vec()
    }

    /// IRIs proved unsatisfiable (⊑ ⊥). Sorted ascending.
    #[getter]
    fn unsatisfiable(&self) -> Vec<String> {
        self.inner
            .unsatisfiable_classes()
            .into_iter()
            .map(String::from)
            .collect()
    }

    /// True iff the whole ontology was flagged inconsistent.
    /// (Set by the Phase-A1 ABox consistency check.)
    #[getter]
    fn inconsistent(&self) -> bool {
        self.inner.stats().inconsistent
    }

    /// True iff `sub ⊑ sup` is entailed.
    fn is_subclass(&self, sub: &str, sup: &str) -> bool {
        self.inner.is_subclass(sub, sup)
    }

    /// All classes equivalent to `cls` (including `cls` itself).
    fn equivalent_classes(&self, cls: &str) -> Vec<String> {
        self.inner
            .equivalent_classes(cls)
            .into_iter()
            .map(String::from)
            .collect()
    }

    /// The Hasse-direct super-classes of `cls`. (Direct, not transitive.)
    fn direct_subsumers(&self, cls: &str) -> Vec<String> {
        self.inner
            .direct_subsumers(cls)
            .into_iter()
            .map(String::from)
            .collect()
    }

    fn __repr__(&self) -> String {
        format!(
            "Classification(classes={}, unsatisfiable={}, inconsistent={})",
            self.inner.classes().len(),
            self.inner.unsatisfiable_classes().len(),
            self.inner.stats().inconsistent,
        )
    }
}

/// `rustdl.classify(path)` — classify the ontology at `path`.
/// Format auto-detected from the file extension.
#[pyfunction]
#[pyo3(signature = (path, *, per_pair_timeout_ms=None, saturation_only=false))]
pub(crate) fn classify(
    path: &str,
    per_pair_timeout_ms: Option<u64>,
    saturation_only: bool,
) -> PyResult<PyClassification> {
    let ontology = load::load_path(path)?;
    do_classify(&ontology, per_pair_timeout_ms, saturation_only)
}

/// `rustdl.classify_bytes(data, format="ofn")` — same but from bytes.
#[pyfunction]
#[pyo3(signature = (data, *, format, per_pair_timeout_ms=None, saturation_only=false))]
pub(crate) fn classify_bytes(
    data: &[u8],
    format: &str,
    per_pair_timeout_ms: Option<u64>,
    saturation_only: bool,
) -> PyResult<PyClassification> {
    let ontology = load::load_bytes(data, format)?;
    do_classify(&ontology, per_pair_timeout_ms, saturation_only)
}

fn do_classify(
    ontology: &horned_owl::ontology::set::SetOntology<horned_owl::model::RcStr>,
    per_pair_timeout_ms: Option<u64>,
    saturation_only: bool,
) -> PyResult<PyClassification> {
    use std::time::Duration;
    let inner = if saturation_only {
        owl_dl_reasoner::classify_saturation_only(ontology).map_err(reason_error_to_py)?
    } else if let Some(ms) = per_pair_timeout_ms {
        owl_dl_reasoner::classify_top_down_with_timeout(ontology, Duration::from_millis(ms))
            .map_err(reason_error_to_py)?
    } else {
        rs_classify(ontology).map_err(reason_error_to_py)?
    };
    Ok(PyClassification { inner })
}

/// Register module bindings.
pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyClassification>()?;
    m.add_function(wrap_pyfunction!(classify, m)?)?;
    m.add_function(wrap_pyfunction!(classify_bytes, m)?)?;
    Ok(())
}
```

> **API verification reminder:** `owl_dl_reasoner::classify` returns `Result<Classification, ReasonError>`. `classify_saturation_only` and `classify_top_down_with_timeout` exist per the recon (see `crates/owl-dl-reasoner/src/classify.rs:340/416/826`). `ClassificationStats::inconsistent` was added in Phase A1 (see `crates/owl-dl-reasoner/src/classify.rs:116+`).

- [ ] **Step 4: Wire into `src/lib.rs`**

```rust
mod classify;
mod errors;
mod load;

#[pymodule]
fn rustdl(_py: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    errors::register(m)?;
    classify::register(m)?;
    Ok(())
}
```

- [ ] **Step 5: Rebuild + re-run tests**

```sh
cd crates/owl-dl-py && maturin develop --release 2>&1 | tail -3 && cd ../..
source /tmp/rustdl-py-venv/bin/activate
pytest crates/owl-dl-py/tests/python/test_classify.py -v 2>&1 | tail -10
```

Expected: all 4 tests pass.

- [ ] **Step 6: Clippy**

```sh
cargo clippy -p owl-dl-py --all-targets -- -D warnings 2>&1 | tail -5
```

Expected: no new warnings on owl-dl-py. PyO3 macros may need `#[allow(clippy::needless_pass_by_value)]` or similar — add minimally if needed.

- [ ] **Step 7: Commit**

```sh
git add crates/owl-dl-py/src/classify.rs crates/owl-dl-py/src/lib.rs \
        crates/owl-dl-py/tests/python/conftest.py \
        crates/owl-dl-py/tests/python/test_classify.py
git restore --staged .claude/settings.json 2>/dev/null || true
git commit -m "$(cat <<'EOF'
feat(py): T5 — classify + classify_bytes + Classification class

Top-level rustdl.classify(path) and rustdl.classify_bytes(data,
format=) functions with optional per_pair_timeout_ms / saturation_only
keyword args. Classification PyO3 class exposes: classes,
unsatisfiable, inconsistent (properties); is_subclass(),
equivalent_classes(), direct_subsumers() (methods). 4 pytest
integration tests cover the happy path + unknown-extension error.

Spec: docs/superpowers/specs/2026-06-04-python-bindings-design.md
EOF
)"
```

---

### Task 6: Python-side `subclasses_of` / `superclasses_of` helpers

**Files:**
- Modify: `crates/owl-dl-py/python/rustdl/__init__.py`
- Test: `crates/owl-dl-py/tests/python/test_classify.py` (append)

These two methods aren't on the Rust `Classification` — synthesize them in pure Python by iterating `classes()` and filtering via `is_subclass`. O(N) per call; acceptable for the API surface.

- [ ] **Step 1: Write failing tests**

Append to `crates/owl-dl-py/tests/python/test_classify.py`:

```python
def test_subclasses_of(fixtures_dir):
    fixture = fixtures_dir / "datatype" / "datatype_definition.ofn"
    result = rustdl.classify(str(fixture))
    # Person should have at least Adult as a (proper or reflexive) subclass
    subs = result.subclasses_of("http://t/Person")
    assert "http://t/Adult" in subs


def test_superclasses_of(fixtures_dir):
    fixture = fixtures_dir / "datatype" / "datatype_definition.ofn"
    result = rustdl.classify(str(fixture))
    sups = result.superclasses_of("http://t/Adult")
    assert "http://t/Person" in sups
```

Run:
```sh
pytest crates/owl-dl-py/tests/python/test_classify.py::test_subclasses_of -v 2>&1 | tail -5
```

Expected: fails with `AttributeError: 'rustdl.Classification' object has no attribute 'subclasses_of'`.

- [ ] **Step 2: Monkey-patch the helpers onto Classification at import time**

Replace `crates/owl-dl-py/python/rustdl/__init__.py`:

```python
"""
rustdl — sound, performant OWL 2 DL (SROIQ) reasoner.

Python bindings for the rustdl Rust crate. Install via
`pip install rustdl`; import as `import rustdl`. See
https://github.com/MaastrichtU-IDS/rustdl for the full project.
"""

# Native extension built by PyO3 + maturin
from rustdl._native import (
    __version__ as __version__,
    Classification as Classification,
    classify as classify,
    classify_bytes as classify_bytes,
    RustdlError as RustdlError,
    ParseError as ParseError,
    UnsupportedAxiomError as UnsupportedAxiomError,
    UnknownClassError as UnknownClassError,
)


def _subclasses_of(self: "Classification", cls: str) -> list[str]:
    """All classes D in the ontology with D ⊑ cls (reflexive + proper).

    Pure-Python helper. O(N) over Classification.classes per call.
    """
    return [d for d in self.classes if self.is_subclass(d, cls)]


def _superclasses_of(self: "Classification", cls: str) -> list[str]:
    """All classes D in the ontology with cls ⊑ D (reflexive + proper).

    Pure-Python helper. O(N) over Classification.classes per call.
    """
    return [d for d in self.classes if self.is_subclass(cls, d)]


# Bind onto the PyO3 class so the API is symmetric:
# `result.subclasses_of(...)` lives next to `result.is_subclass(...)`.
Classification.subclasses_of = _subclasses_of  # type: ignore[attr-defined]
Classification.superclasses_of = _superclasses_of  # type: ignore[attr-defined]


__all__ = [
    "__version__",
    "Classification",
    "classify",
    "classify_bytes",
    "RustdlError",
    "ParseError",
    "UnsupportedAxiomError",
    "UnknownClassError",
]
```

- [ ] **Step 3: Rebuild + re-run tests**

```sh
cd crates/owl-dl-py && maturin develop --release 2>&1 | tail -3 && cd ../..
pytest crates/owl-dl-py/tests/python/test_classify.py -v 2>&1 | tail -10
```

Expected: all 6 tests pass (the original 4 + 2 new).

- [ ] **Step 4: Commit**

```sh
git add crates/owl-dl-py/python/rustdl/__init__.py crates/owl-dl-py/tests/python/test_classify.py
git restore --staged .claude/settings.json 2>/dev/null || true
git commit -m "$(cat <<'EOF'
feat(py): T6 — subclasses_of + superclasses_of helpers

Pure-Python wrappers around Classification.is_subclass that iterate
the classes list. Bound onto the PyO3 class at import time so the API
surface is symmetric. O(N) per call — acceptable for typical workloads.

Spec: docs/superpowers/specs/2026-06-04-python-bindings-design.md
EOF
)"
```

---

## Phase 4: Other query bindings

### Task 7: `is_consistent`, `is_class_satisfiable`, `is_subclass_of`

**Files:**
- Create: `crates/owl-dl-py/src/queries.rs`
- Modify: `crates/owl-dl-py/src/lib.rs` + `crates/owl-dl-py/python/rustdl/__init__.py`
- Test: `crates/owl-dl-py/tests/python/test_queries.py`

- [ ] **Step 1: Write failing tests**

`crates/owl-dl-py/tests/python/test_queries.py`:

```python
import rustdl
import pytest


def test_is_consistent_true(fixtures_dir):
    fixture = fixtures_dir / "datatype" / "datatype_definition.ofn"
    assert rustdl.is_consistent(str(fixture)) is True


def test_is_class_satisfiable_true(fixtures_dir):
    fixture = fixtures_dir / "datatype" / "datatype_definition.ofn"
    assert rustdl.is_class_satisfiable(str(fixture), "http://t/Person") is True


def test_is_subclass_of_direct(fixtures_dir):
    fixture = fixtures_dir / "datatype" / "datatype_definition.ofn"
    assert rustdl.is_subclass_of(str(fixture), "http://t/Adult", "http://t/Person") is True


def test_is_class_satisfiable_unknown_class_raises(fixtures_dir):
    fixture = fixtures_dir / "datatype" / "datatype_definition.ofn"
    with pytest.raises(rustdl.UnknownClassError):
        rustdl.is_class_satisfiable(str(fixture), "http://t/NonExistent")
```

Run:
```sh
pytest crates/owl-dl-py/tests/python/test_queries.py -v 2>&1 | tail -8
```

Expected: fails — `module 'rustdl' has no attribute 'is_consistent'`.

- [ ] **Step 2: Create `crates/owl-dl-py/src/queries.rs`**

```rust
//! Top-level query bindings: consistency, satisfiability, subsumption,
//! instance checks, realization.

use pyo3::prelude::*;

use crate::errors::reason_error_to_py;
use crate::load;

#[pyfunction]
pub(crate) fn is_consistent(path: &str) -> PyResult<bool> {
    let ontology = load::load_path(path)?;
    owl_dl_reasoner::is_consistent(&ontology).map_err(reason_error_to_py)
}

#[pyfunction]
pub(crate) fn is_class_satisfiable(path: &str, class_iri: &str) -> PyResult<bool> {
    let ontology = load::load_path(path)?;
    owl_dl_reasoner::is_class_satisfiable(&ontology, class_iri).map_err(reason_error_to_py)
}

#[pyfunction]
pub(crate) fn is_subclass_of(path: &str, sub: &str, sup: &str) -> PyResult<bool> {
    let ontology = load::load_path(path)?;
    owl_dl_reasoner::is_subclass_of(&ontology, sub, sup).map_err(reason_error_to_py)
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(is_consistent, m)?)?;
    m.add_function(wrap_pyfunction!(is_class_satisfiable, m)?)?;
    m.add_function(wrap_pyfunction!(is_subclass_of, m)?)?;
    Ok(())
}
```

> **API check:** these three functions exist on `owl_dl_reasoner`. Confirm with `grep -n "^pub fn is_" crates/owl-dl-reasoner/src/lib.rs`. The recon showed `is_consistent`, `is_class_satisfiable`, and `is_subclass_of` are all in `lib.rs` returning `Result<bool, ReasonError>`.

- [ ] **Step 3: Wire into `src/lib.rs`**

Add `mod queries;` and call `queries::register(m)?;` in the pymodule body.

- [ ] **Step 4: Add to `python/rustdl/__init__.py`**

Extend the re-export block:

```python
from rustdl._native import (
    __version__ as __version__,
    Classification as Classification,
    classify as classify,
    classify_bytes as classify_bytes,
    is_consistent as is_consistent,
    is_class_satisfiable as is_class_satisfiable,
    is_subclass_of as is_subclass_of,
    RustdlError as RustdlError,
    ParseError as ParseError,
    UnsupportedAxiomError as UnsupportedAxiomError,
    UnknownClassError as UnknownClassError,
)
```

And add the new names to `__all__`.

- [ ] **Step 5: Rebuild + re-run tests**

```sh
cd crates/owl-dl-py && maturin develop --release 2>&1 | tail -3 && cd ../..
pytest crates/owl-dl-py/tests/python/test_queries.py -v 2>&1 | tail -10
```

Expected: all 4 tests pass.

- [ ] **Step 6: Commit**

```sh
git add crates/owl-dl-py/src/queries.rs crates/owl-dl-py/src/lib.rs \
        crates/owl-dl-py/python/rustdl/__init__.py \
        crates/owl-dl-py/tests/python/test_queries.py
git restore --staged .claude/settings.json 2>/dev/null || true
git commit -m "$(cat <<'EOF'
feat(py): T7 — is_consistent + is_class_satisfiable + is_subclass_of

Three top-level query functions, one-to-one with the owl-dl-reasoner
public API. 4 pytest tests cover happy path + UnknownClass error.

Spec: docs/superpowers/specs/2026-06-04-python-bindings-design.md
EOF
)"
```

---

### Task 8: `is_instance_of`, `instances_of`, `realize`

**Files:**
- Modify: `crates/owl-dl-py/src/queries.rs` (append)
- Modify: `crates/owl-dl-py/python/rustdl/__init__.py`
- Test: `crates/owl-dl-py/tests/python/test_queries.py` (append)

- [ ] **Step 1: Write failing tests**

Append to `crates/owl-dl-py/tests/python/test_queries.py`:

```python
def test_is_instance_of_simple(fixtures_dir):
    # Use a fixture that has a ClassAssertion
    fixture = fixtures_dir / "abox" / "p1_direct_bot.ofn"
    # p1_direct_bot has ClassAssertion(:Unsat :a)
    assert rustdl.is_instance_of(str(fixture), "http://t/Unsat", "http://t/a") is True


def test_instances_of_simple(fixtures_dir):
    fixture = fixtures_dir / "abox" / "p1_direct_bot.ofn"
    instances = rustdl.instances_of(str(fixture), "http://t/Unsat")
    assert "http://t/a" in instances


def test_realize_returns_dict(fixtures_dir):
    fixture = fixtures_dir / "abox" / "p1_direct_bot.ofn"
    realization = rustdl.realize(str(fixture))
    assert isinstance(realization, dict)
    assert "http://t/a" in realization
    assert isinstance(realization["http://t/a"], list)
```

Run:
```sh
pytest crates/owl-dl-py/tests/python/test_queries.py -v 2>&1 | tail -5
```

Expected: three new tests fail with `AttributeError`.

- [ ] **Step 2: Append to `crates/owl-dl-py/src/queries.rs`**

Add these functions to the existing file:

```rust
use std::collections::HashMap;

#[pyfunction]
pub(crate) fn is_instance_of(path: &str, class_iri: &str, individual_iri: &str) -> PyResult<bool> {
    let ontology = load::load_path(path)?;
    owl_dl_reasoner::is_instance_of(&ontology, class_iri, individual_iri)
        .map_err(reason_error_to_py)
}

#[pyfunction]
pub(crate) fn instances_of(path: &str, class_iri: &str) -> PyResult<Vec<String>> {
    let ontology = load::load_path(path)?;
    owl_dl_reasoner::instances_of(&ontology, class_iri).map_err(reason_error_to_py)
}

#[pyfunction]
pub(crate) fn realize(path: &str) -> PyResult<HashMap<String, Vec<String>>> {
    let ontology = load::load_path(path)?;
    let rs_realization = owl_dl_reasoner::realize(&ontology).map_err(reason_error_to_py)?;
    Ok(realization_to_dict(&rs_realization))
}

fn realization_to_dict(
    realization: &owl_dl_reasoner::Realization,
) -> HashMap<String, Vec<String>> {
    // Realization's public API: iterate individuals, get most-specific
    // types for each. Adapt to whatever method exists on the type;
    // grep `crates/owl-dl-reasoner/src/realize.rs` for the public API
    // (`individuals()`, `types_of()` or similar).
    realization
        .individuals()
        .iter()
        .map(|ind| {
            (
                (*ind).to_string(),
                realization
                    .most_specific_types(ind)
                    .iter()
                    .map(|s| (*s).to_string())
                    .collect(),
            )
        })
        .collect()
}
```

> **API verification reminder:** the actual `Realization` API may use different method names (e.g., `types_of` vs `most_specific_types`). The implementer should `grep "^pub fn" crates/owl-dl-reasoner/src/realize.rs` and `grep "^pub struct Realization\|^impl Realization" crates/owl-dl-reasoner/src/realize.rs` to confirm. If a method's signature differs, adapt the body but keep the function's external Python signature (return type `dict[str, list[str]]`).

Extend `register` to add the three new functions:

```rust
pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(is_consistent, m)?)?;
    m.add_function(wrap_pyfunction!(is_class_satisfiable, m)?)?;
    m.add_function(wrap_pyfunction!(is_subclass_of, m)?)?;
    m.add_function(wrap_pyfunction!(is_instance_of, m)?)?;
    m.add_function(wrap_pyfunction!(instances_of, m)?)?;
    m.add_function(wrap_pyfunction!(realize, m)?)?;
    Ok(())
}
```

- [ ] **Step 3: Extend `python/rustdl/__init__.py`**

Add three more re-exports:

```python
    is_instance_of as is_instance_of,
    instances_of as instances_of,
    realize as realize,
```

And add to `__all__`.

- [ ] **Step 4: Rebuild + re-run**

```sh
cd crates/owl-dl-py && maturin develop --release 2>&1 | tail -3 && cd ../..
pytest crates/owl-dl-py/tests/python/test_queries.py -v 2>&1 | tail -10
```

Expected: all 7 tests pass.

- [ ] **Step 5: Commit**

```sh
git add crates/owl-dl-py/src/queries.rs crates/owl-dl-py/python/rustdl/__init__.py \
        crates/owl-dl-py/tests/python/test_queries.py
git restore --staged .claude/settings.json 2>/dev/null || true
git commit -m "$(cat <<'EOF'
feat(py): T8 — is_instance_of + instances_of + realize

Three ABox query functions. realize returns dict[str, list[str]]:
{individual_iri: [most_specific_type_iri, ...]}. Reuses the
owl-dl-reasoner ABox query API one-to-one.

Spec: docs/superpowers/specs/2026-06-04-python-bindings-design.md
EOF
)"
```

---

## Phase 5: Materialization helpers

### Task 9: `materialize_inferred_subclass_axioms` + `materialize_inferred_class_assertions`

**Files:**
- Create: `crates/owl-dl-py/src/materialize.rs`
- Modify: `crates/owl-dl-py/src/lib.rs` + `python/rustdl/__init__.py`
- Test: `crates/owl-dl-py/tests/python/test_materialize.py`

- [ ] **Step 1: Write failing tests**

`crates/owl-dl-py/tests/python/test_materialize.py`:

```python
import rustdl


def test_materialize_inferred_subclass_axioms(fixtures_dir):
    fixture = fixtures_dir / "datatype" / "datatype_definition.ofn"
    axioms = rustdl.materialize_inferred_subclass_axioms(str(fixture))
    assert isinstance(axioms, list)
    assert all(isinstance(a, tuple) and len(a) == 2 for a in axioms)
    # Adult ⊑ Person should appear
    assert ("http://t/Adult", "http://t/Person") in axioms


def test_materialize_inferred_class_assertions(fixtures_dir):
    fixture = fixtures_dir / "abox" / "p1_direct_bot.ofn"
    axioms = rustdl.materialize_inferred_class_assertions(str(fixture))
    assert isinstance(axioms, list)
    assert all(isinstance(a, tuple) and len(a) == 2 for a in axioms)
    # (class, individual) for ClassAssertion(:Unsat :a)
    assert ("http://t/Unsat", "http://t/a") in axioms
```

Run + see failures.

- [ ] **Step 2: Create `crates/owl-dl-py/src/materialize.rs`**

```rust
//! Inference-materialization helpers — derive lists of inferred
//! axioms from classify/realize results. Reuses the existing
//! reasoner; no engine changes.

use pyo3::prelude::*;

use crate::errors::reason_error_to_py;
use crate::load;

/// Returns every (sub, sup) class IRI pair `(sub, sup)` such that
/// `sub ⊑ sup` is entailed. Excludes:
/// - Reflexive pairs (`sub == sup`)
/// - Pairs involving `owl:Thing` or `owl:Nothing`
/// - Pairs from unsatisfiable classes (which trivially subsume all)
#[pyfunction]
pub(crate) fn materialize_inferred_subclass_axioms(
    path: &str,
) -> PyResult<Vec<(String, String)>> {
    let ontology = load::load_path(path)?;
    let classification = owl_dl_reasoner::classify(&ontology).map_err(reason_error_to_py)?;
    let classes = classification.classes();
    let unsat: std::collections::HashSet<&str> = classification
        .unsatisfiable_classes()
        .into_iter()
        .collect();
    let mut out = Vec::new();
    for sub in classes {
        if unsat.contains(sub.as_str()) {
            continue;
        }
        if sub == "http://www.w3.org/2002/07/owl#Thing"
            || sub == "http://www.w3.org/2002/07/owl#Nothing"
        {
            continue;
        }
        for sup in classes {
            if sub == sup {
                continue;
            }
            if sup == "http://www.w3.org/2002/07/owl#Thing"
                || sup == "http://www.w3.org/2002/07/owl#Nothing"
            {
                continue;
            }
            if classification.is_subclass(sub, sup) {
                out.push((sub.clone(), sup.clone()));
            }
        }
    }
    Ok(out)
}

/// Returns every (class IRI, individual IRI) pair `(c, i)` such that
/// `ClassAssertion(c, i)` is entailed.
#[pyfunction]
pub(crate) fn materialize_inferred_class_assertions(
    path: &str,
) -> PyResult<Vec<(String, String)>> {
    let ontology = load::load_path(path)?;
    let realization = owl_dl_reasoner::realize(&ontology).map_err(reason_error_to_py)?;
    let mut out = Vec::new();
    for ind in realization.individuals() {
        for c in realization.most_specific_types(ind) {
            out.push((c.to_string(), ind.to_string()));
        }
    }
    Ok(out)
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(materialize_inferred_subclass_axioms, m)?)?;
    m.add_function(wrap_pyfunction!(materialize_inferred_class_assertions, m)?)?;
    Ok(())
}
```

> **Same API caveat as T8:** verify `Realization::individuals()` and `most_specific_types(ind)`. If method names differ, adapt while keeping the Python signature.

- [ ] **Step 3: Wire + register + re-export**

In `src/lib.rs`: `mod materialize;` + `materialize::register(m)?;`.

In `python/rustdl/__init__.py`: add to the `from rustdl._native import (...)` block:

```python
    materialize_inferred_subclass_axioms as materialize_inferred_subclass_axioms,
    materialize_inferred_class_assertions as materialize_inferred_class_assertions,
```

Add both names to `__all__`.

- [ ] **Step 4: Rebuild + run**

```sh
cd crates/owl-dl-py && maturin develop --release 2>&1 | tail -3 && cd ../..
pytest crates/owl-dl-py/tests/python/test_materialize.py -v 2>&1 | tail -5
```

Expected: both tests pass.

- [ ] **Step 5: Commit**

```sh
git add crates/owl-dl-py/src/materialize.rs crates/owl-dl-py/src/lib.rs \
        crates/owl-dl-py/python/rustdl/__init__.py \
        crates/owl-dl-py/tests/python/test_materialize.py
git restore --staged .claude/settings.json 2>/dev/null || true
git commit -m "$(cat <<'EOF'
feat(py): T9 — inference materialization helpers

materialize_inferred_subclass_axioms(path) returns list of (sub, sup)
class IRI pairs (excludes reflexive, owl:Thing/Nothing, unsat-class
pairs). materialize_inferred_class_assertions(path) returns list of
(class, individual) IRI pairs from realize().

Spec: docs/superpowers/specs/2026-06-04-python-bindings-design.md
EOF
)"
```

---

## Phase 6: Soundness regression

### Task 10: Soundness regression test — alehif closure-diff through Python

**Files:**
- Test: `crates/owl-dl-py/tests/python/test_soundness.py`

This test validates that classifying alehif through the Python bindings produces the same closure set as the Rust-side closure-diff test. Pure regression guard against the bindings dropping or corrupting data.

- [ ] **Step 1: Write the test**

`crates/owl-dl-py/tests/python/test_soundness.py`:

```python
"""Soundness regression — Python bindings preserve FP=0 vs Konclude on alehif.

Mirrors crates/owl-dl-reasoner/tests/konclude_closure_diff.rs
::alehif_closure_matches_konclude, but invokes through the Python
bindings. If this test fails, the bindings have introduced a data
corruption between Rust output and the Python return value — STOP
and investigate before releasing.
"""

import pathlib
import pytest
import rustdl

OWL_THING = "http://www.w3.org/2002/07/owl#Thing"
OWL_NOTHING = "http://www.w3.org/2002/07/owl#Nothing"


@pytest.mark.skipif(
    not (pathlib.Path(__file__).resolve().parents[4]
         / "ontologies" / "real" / "alehif-test.ofn").exists(),
    reason="ontologies corpus not fetched (run scripts/fetch-real-ontologies.sh)",
)
def test_alehif_closure_size_through_python(fixtures_dir):
    repo_root = pathlib.Path(__file__).resolve().parents[4]
    onto = repo_root / "ontologies" / "real" / "alehif-test.ofn"
    result = rustdl.classify(str(onto))

    # Count non-trivial subsumption pairs (no owl:Thing/Nothing, no reflexive).
    classes = [
        c for c in result.classes
        if c not in (OWL_THING, OWL_NOTHING)
    ]
    pair_count = sum(
        1
        for sub in classes
        for sup in classes
        if sub != sup and result.is_subclass(sub, sup)
    )

    # alehif's Konclude-confirmed closure = 247 pairs (per docs/perf-2026-06-04).
    # If this drifts, EITHER the Python bindings dropped data OR the
    # Rust-side closure shifted (the corpus closure-diff test guards that;
    # check it before assuming Python is at fault).
    assert pair_count == 247, (
        f"alehif closure through Python = {pair_count}; expected 247. "
        "Either bindings broken OR Rust closure drifted "
        "(check `cargo test -p owl-dl-reasoner --release --test "
        "konclude_closure_diff alehif`)."
    )
```

Note: this test only runs if the corpus is present (typically gitignored, pulled via `scripts/fetch-real-ontologies.sh`). Otherwise it's auto-skipped. CI can run it conditionally; local dev should run it before each release.

- [ ] **Step 2: Run it**

```sh
pytest crates/owl-dl-py/tests/python/test_soundness.py -v 2>&1 | tail -8
```

Expected: passes if the corpus is present; SKIPS otherwise. Both are acceptable.

- [ ] **Step 3: Commit**

```sh
git add crates/owl-dl-py/tests/python/test_soundness.py
git restore --staged .claude/settings.json 2>/dev/null || true
git commit -m "$(cat <<'EOF'
test(py): T10 — alehif soundness regression through Python bindings

Validates classify(alehif) through Python preserves the 247-pair
closure that Konclude confirms. Pure tripwire guarding against
data corruption in the binding layer (between Rust return and
Python return). Skips when the corpus isn't fetched (gitignored).

Spec: docs/superpowers/specs/2026-06-04-python-bindings-design.md
EOF
)"
```

---

## Phase 7: CI workflows

### Task 11: `python-ci.yml` — PR/push gate

**Files:**
- Create: `.github/workflows/python-ci.yml`

- [ ] **Step 1: Write the workflow**

```yaml
name: Python CI

on:
  push:
    branches: [main]
    paths:
      - 'crates/owl-dl-py/**'
      - 'crates/owl-dl-core/**'
      - 'crates/owl-dl-saturation/**'
      - 'crates/owl-dl-tableau/**'
      - 'crates/owl-dl-datatypes/**'
      - 'crates/owl-dl-reasoner/**'
      - 'Cargo.toml'
      - '.github/workflows/python-ci.yml'
  pull_request:

jobs:
  test:
    runs-on: ubuntu-latest
    strategy:
      matrix:
        python-version: ['3.10', '3.13']
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-python@v5
        with:
          python-version: ${{ matrix.python-version }}
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - name: Install maturin + pytest
        run: pip install 'maturin>=1.7,<2' pytest
      - name: Build + install rustdl
        working-directory: crates/owl-dl-py
        run: maturin develop --release
      - name: Run pytest
        run: pytest crates/owl-dl-py/tests/python/ -v
```

- [ ] **Step 2: Commit**

```sh
git add .github/workflows/python-ci.yml
git restore --staged .claude/settings.json 2>/dev/null || true
git commit -m "$(cat <<'EOF'
ci(py): T11 — python-ci.yml — maturin develop + pytest on PR/push

Runs on every PR + push to main. Matrix on Python 3.10 + 3.13
(end-points of the supported range; intermediate versions covered
by abi3 wheel compatibility). Path filter scopes runs to changes in
the workspace crates or workflow definition. Existing cargo CI
remains unchanged.

Spec: docs/superpowers/specs/2026-06-04-python-bindings-design.md
EOF
)"
```

---

### Task 12: `release-python.yml` — cibuildwheel + PyPI publish

**Files:**
- Create: `.github/workflows/release-python.yml`

- [ ] **Step 1: Write the workflow**

```yaml
name: Release Python wheels

on:
  push:
    tags: ['v*.*.*']
  workflow_dispatch:

permissions:
  id-token: write    # required for PyPI trusted publisher OIDC
  contents: read

jobs:
  build-wheels:
    name: Build wheels for ${{ matrix.os }} / ${{ matrix.arch }}
    runs-on: ${{ matrix.os }}
    strategy:
      fail-fast: false
      matrix:
        include:
          - os: ubuntu-latest
            arch: x86_64
          - os: ubuntu-latest
            arch: aarch64
          - os: macos-13
            arch: x86_64
          - os: macos-14
            arch: arm64
          - os: windows-latest
            arch: AMD64
    steps:
      - uses: actions/checkout@v4
      - name: Set up QEMU (for aarch64 cross-build on x86_64 host)
        if: matrix.arch == 'aarch64'
        uses: docker/setup-qemu-action@v3
      - uses: pypa/cibuildwheel@v2.21
        env:
          CIBW_BUILD: 'cp310-*'   # abi3 wheel — one build, all 3.10+ Pythons
          CIBW_ARCHS: ${{ matrix.arch }}
          CIBW_BEFORE_BUILD: 'pip install maturin'
          CIBW_BUILD_FRONTEND: 'build'
          CIBW_TEST_REQUIRES: 'pytest'
          CIBW_TEST_COMMAND: 'pytest {project}/crates/owl-dl-py/tests/python/'
        with:
          package-dir: crates/owl-dl-py
          output-dir: wheelhouse
      - uses: actions/upload-artifact@v4
        with:
          name: wheels-${{ matrix.os }}-${{ matrix.arch }}
          path: wheelhouse/*.whl

  build-sdist:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-python@v5
        with:
          python-version: '3.12'
      - run: pip install 'maturin>=1.7,<2'
      - name: Build sdist
        working-directory: crates/owl-dl-py
        run: maturin sdist
      - uses: actions/upload-artifact@v4
        with:
          name: sdist
          path: crates/owl-dl-py/target/wheels/*.tar.gz

  publish-pypi:
    name: Publish to PyPI
    needs: [build-wheels, build-sdist]
    runs-on: ubuntu-latest
    environment:
      name: pypi
      url: https://pypi.org/project/rustdl/
    permissions:
      id-token: write
    steps:
      - uses: actions/download-artifact@v4
        with:
          pattern: 'wheels-*'
          path: dist
          merge-multiple: true
      - uses: actions/download-artifact@v4
        with:
          name: sdist
          path: dist
      - uses: pypa/gh-action-pypi-publish@release/v1
        with:
          packages-dir: dist
          # No password / API token — PyPI trusted publisher OIDC
```

- [ ] **Step 2: Note the one-time PyPI setup the user has to do**

After this commit lands, the user must do TWO one-time things on PyPI before the first publish actually works:

1. Create the project `rustdl` on PyPI (or use a pending publisher for first publish). https://pypi.org/manage/account/publishing/
2. Register the GitHub Actions workflow as a trusted publisher:
   - Owner: `MaastrichtU-IDS`
   - Repository: `rustdl`
   - Workflow filename: `release-python.yml`
   - Environment name: `pypi` (must match the `environment.name` in the workflow)

Document this in the commit message + handoff so the user doesn't forget.

- [ ] **Step 3: Commit**

```sh
git add .github/workflows/release-python.yml
git restore --staged .claude/settings.json 2>/dev/null || true
git commit -m "$(cat <<'EOF'
ci(py): T12 — release-python.yml — cibuildwheel + PyPI trusted publisher

On v*.*.* tag (or manual dispatch): builds wheels for 5 platforms
(Linux x86_64 / aarch64 via QEMU, macOS x86_64 / arm64, Windows
AMD64) + sdist. Each wheel is abi3-py310 so one build supports
Python 3.10/3.11/3.12/3.13. cibuildwheel runs pytest inside each
wheel as a smoke test. Publishes via PyPI trusted publisher OIDC
(no token).

ONE-TIME SETUP REQUIRED before the first publish actually works:
1. Create `rustdl` project on PyPI (or use pending-publisher for first publish).
2. https://pypi.org/manage/account/publishing/ — add trusted publisher:
   - Owner: MaastrichtU-IDS
   - Repository: rustdl
   - Workflow: release-python.yml
   - Environment: pypi

Spec: docs/superpowers/specs/2026-06-04-python-bindings-design.md
EOF
)"
```

---

## Phase 8: Documentation

### Task 13: Update `README.md` + `CHANGELOG.md`

**Files:**
- Modify: `README.md` (add Python section)
- Modify: `CHANGELOG.md` (add 0.1.1 or 0.2.0 entry)

- [ ] **Step 1: Add a Python section to `README.md`**

Find the existing `## Install` section. Add a sub-section AFTER it (or alongside the cargo install snippets):

```markdown
### Python

`rustdl` is also available on PyPI:

```sh
pip install rustdl
```

Quick example:

```python
import rustdl

# Classify an ontology (OFN, OWX, or RDF/XML — format auto-detected from extension)
result = rustdl.classify("ontology.ofn")

print(f"{len(result.classes)} classes; {len(result.unsatisfiable)} unsat")
print(result.is_subclass("http://example.org/Sub", "http://example.org/Sup"))

# Other queries
ok = rustdl.is_consistent("ontology.ofn")
instances = rustdl.instances_of("ontology.ofn", "http://example.org/Person")
realization = rustdl.realize("ontology.ofn")  # dict[individual_iri, [most_specific_type, ...]]

# Inference materialization (useful for writing inferred ontologies back to disk)
sub_axioms = rustdl.materialize_inferred_subclass_axioms("ontology.ofn")
type_axioms = rustdl.materialize_inferred_class_assertions("ontology.ofn")
```

Supports Python 3.10+. ABI3 wheel — one wheel per platform for all 3.10–3.13.

```

- [ ] **Step 2: Add CHANGELOG entry**

Decide version: if the Python bindings ship in the next patch (0.1.1), use that; if you want them as a minor bump for visibility, use 0.2.0. The spec implies bindings ride the workspace version. Since the workspace is at 0.1.0 and adding Python bindings is a feature (new functionality, not just a bug fix), **bump to 0.2.0**.

For now in the plan: add an `[Unreleased]` section to the CHANGELOG (don't commit a version bump yet — that's a separate decision when the next release goes out).

Insert in `CHANGELOG.md` immediately after the heading `# Changelog` block and before `## [0.1.0]`:

```markdown
## [Unreleased]

### Added

- **Python bindings** (`rustdl` on PyPI). PyO3 + maturin. ABI3 wheel
  for Python 3.10/3.11/3.12/3.13. Top-level API one-to-one with the
  Rust public API (`classify`, `classify_bytes`, `is_consistent`,
  `is_class_satisfiable`, `is_subclass_of`, `is_instance_of`,
  `instances_of`, `realize`) plus inference materialization helpers
  (`materialize_inferred_subclass_axioms`,
  `materialize_inferred_class_assertions`). Auto-detects OFN/OWX/RDF-XML
  format from file extension. 5-platform wheel matrix (Linux x86_64 +
  aarch64, macOS x86_64 + arm64, Windows AMD64) + sdist. PyPI publish
  via trusted publisher (OIDC, no token in CI).
- New GitHub Actions workflows: `python-ci.yml` (PR/push gate) and
  `release-python.yml` (cibuildwheel + maturin publish on `v*.*.*` tag).

### Deferred to roadmap

- owlready2 / omny integration (separate brainstorm queued).
- Black-box `rustdl.explain(path, sub, sup)` axiom-justifications.
- `rustdl.Reasoner(path)` stateful class for batch queries.
- Native pyhornedowl `Ontology` pass-through.
- See the spec at `docs/superpowers/specs/2026-06-04-python-bindings-design.md`
  for the full deferred-feature list.
```

- [ ] **Step 3: Commit**

```sh
git add README.md CHANGELOG.md
git restore --staged .claude/settings.json 2>/dev/null || true
git commit -m "$(cat <<'EOF'
docs(py): T13 — README install/example section + CHANGELOG Unreleased

README gains a Python install + minimal-usage section. CHANGELOG
adds an [Unreleased] entry covering the bindings, materialization
helpers, and CI workflows. Version bump is deferred to the actual
release (likely 0.2.0 since this is a feature addition, not a
patch).

Spec: docs/superpowers/specs/2026-06-04-python-bindings-design.md
EOF
)"
```

---

## End-state checklist

- [ ] All 14 tasks committed (T0 baseline through T13 docs).
- [ ] `cargo build -p owl-dl-py --release` succeeds.
- [ ] `maturin develop --release` in `crates/owl-dl-py/` produces an importable module.
- [ ] `python -c "import rustdl; print(rustdl.__version__)"` prints `0.1.0`.
- [ ] `pytest crates/owl-dl-py/tests/python/ -v` passes all unit/integration tests (soundness regression may SKIP if corpus not present).
- [ ] `cargo clippy -p owl-dl-py --all-targets -- -D warnings` clean.
- [ ] `cargo test -p owl-dl-reasoner --release --lib | grep "test result"` still shows `83 passed; 6 failed` (no regression).
- [ ] `.github/workflows/python-ci.yml` and `.github/workflows/release-python.yml` exist.
- [ ] `README.md` has a Python install + example section.
- [ ] `CHANGELOG.md` has an `[Unreleased]` entry covering the bindings.

## Out-of-scope (do NOT do in this plan)

- Actually publish to PyPI (requires version bump + tag + the one-time PyPI trusted-publisher setup; the user authorizes these explicitly).
- Push to remote (`git push origin main vX.X.X`).
- owlready2 / omny integration package (separate spec).
- Black-box `explain()`, meta-explain, stateful Reasoner, native pyhornedowl pass-through — all deferred per the spec's roadmap.

## After this plan ships, the user owns

1. The one-time PyPI trusted-publisher registration (T12 commit message has the steps).
2. Decide release version (likely 0.2.0 — feature addition) and bump the workspace `version` in root `Cargo.toml` (6 sites, same pattern as the 0.1.0 release).
3. `git push origin main` to publish the work + `git tag vX.X.X && git push origin vX.X.X` to trigger the release workflow.
4. After the release workflow finishes successfully, verify the package at https://pypi.org/project/rustdl/.
