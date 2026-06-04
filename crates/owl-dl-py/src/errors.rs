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

use owl_dl_core::ConversionError;
use owl_dl_reasoner::ReasonError;
use pyo3::create_exception;
use pyo3::exceptions::PyException;
use pyo3::prelude::*;

create_exception!(rustdl, RustdlError, PyException, "Base exception for all rustdl errors.");
create_exception!(rustdl, ParseError, RustdlError, "OWL parser failure.");
create_exception!(
    rustdl,
    UnsupportedAxiomError,
    RustdlError,
    "Axiom or class expression rustdl can't represent."
);
create_exception!(
    rustdl,
    UnknownClassError,
    RustdlError,
    "Class IRI not declared in the ontology."
);

/// Register all rustdl exception types on the module so Python
/// callers can `except rustdl.RustdlError:` etc.
pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("RustdlError", m.py().get_type_bound::<RustdlError>())?;
    m.add("ParseError", m.py().get_type_bound::<ParseError>())?;
    m.add(
        "UnsupportedAxiomError",
        m.py().get_type_bound::<UnsupportedAxiomError>(),
    )?;
    m.add(
        "UnknownClassError",
        m.py().get_type_bound::<UnknownClassError>(),
    )?;
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
