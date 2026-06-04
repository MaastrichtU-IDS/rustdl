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
fn _native(_py: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    Ok(())
}
