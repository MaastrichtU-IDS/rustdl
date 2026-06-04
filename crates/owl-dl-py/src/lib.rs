//! Python bindings for the rustdl OWL DL reasoner.
//!
//! Built with PyO3 + maturin. Distributed on PyPI as `rustdl`;
//! imported in Python as `import rustdl`. See the spec at
//! `docs/superpowers/specs/2026-06-04-python-bindings-design.md`.

use pyo3::prelude::*;

mod errors;
mod load;

#[pymodule]
fn _native(_py: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    errors::register(m)?;
    Ok(())
}
