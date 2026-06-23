//! Top-level query bindings: consistency, satisfiability, subsumption,
//! instance checks, realization.

use std::collections::HashMap;

use pyo3::prelude::*;

use crate::errors::reason_error_to_py;
use crate::load;

/// True iff the ontology at `path` is consistent.
#[pyfunction]
pub(crate) fn is_consistent(path: &str) -> PyResult<bool> {
    let ontology = load::load_path(path)?;
    owl_dl_reasoner::is_consistent(&ontology).map_err(reason_error_to_py)
}

/// True iff `class_iri` is satisfiable (not ⊑ ⊥).
#[pyfunction]
pub(crate) fn is_class_satisfiable(path: &str, class_iri: &str) -> PyResult<bool> {
    let ontology = load::load_path(path)?;
    owl_dl_reasoner::is_class_satisfiable(&ontology, class_iri).map_err(reason_error_to_py)
}

/// True iff `sub ⊑ sup` is entailed.
#[pyfunction]
pub(crate) fn is_subclass_of(path: &str, sub: &str, sup: &str) -> PyResult<bool> {
    let ontology = load::load_path(path)?;
    owl_dl_reasoner::is_subclass_of(&ontology, sub, sup).map_err(reason_error_to_py)
}

/// True iff `individual_iri` is an instance of `class_iri`.
#[pyfunction]
pub(crate) fn is_instance_of(path: &str, class_iri: &str, individual_iri: &str) -> PyResult<bool> {
    let ontology = load::load_path(path)?;
    owl_dl_reasoner::is_instance_of(&ontology, class_iri, individual_iri)
        .map_err(reason_error_to_py)
}

/// Named individuals entailed to be instances of `class_iri`.
#[pyfunction]
pub(crate) fn instances_of(path: &str, class_iri: &str) -> PyResult<Vec<String>> {
    let ontology = load::load_path(path)?;
    owl_dl_reasoner::instances_of(&ontology, class_iri).map_err(reason_error_to_py)
}

/// Map each named individual to its most-specific entailed types.
/// Each instance check routes through the ABox-seeded hypertableau wedge — a
/// sound, terminating, complete decision procedure on `SHOIQ`/`SROIQ`.
/// `per_pair_timeout_ms` bounds each check, mirroring `classify`. The default
/// (100 ms) is FAST: a positive budget is a sound under-approximation
/// (timed-out pairs reported "not an instance", possibly incomplete). Pass `0`
/// for the complete (unbounded) result (safe — the wedge always terminates).
#[pyfunction]
#[pyo3(signature = (path, *, per_pair_timeout_ms=100))]
pub(crate) fn realize(
    path: &str,
    per_pair_timeout_ms: Option<u64>,
) -> PyResult<HashMap<String, Vec<String>>> {
    let ontology = load::load_path(path)?;
    let rs_realization = owl_dl_reasoner::realize_with_timeout(&ontology, per_pair_timeout_ms)
        .map_err(reason_error_to_py)?;
    Ok(realization_to_dict(&rs_realization))
}

fn realization_to_dict(realization: &owl_dl_reasoner::Realization) -> HashMap<String, Vec<String>> {
    realization
        .individuals()
        .iter()
        .map(|ind| (ind.clone(), realization.most_specific_types(ind).to_vec()))
        .collect()
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(is_consistent, m)?)?;
    m.add_function(wrap_pyfunction!(is_class_satisfiable, m)?)?;
    m.add_function(wrap_pyfunction!(is_subclass_of, m)?)?;
    m.add_function(wrap_pyfunction!(is_instance_of, m)?)?;
    m.add_function(wrap_pyfunction!(instances_of, m)?)?;
    m.add_function(wrap_pyfunction!(realize, m)?)?;
    Ok(())
}
