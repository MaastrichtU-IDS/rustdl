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
pub(crate) fn materialize_inferred_subclass_axioms(path: &str) -> PyResult<Vec<(String, String)>> {
    let ontology = load::load_path(path)?;
    let classification = owl_dl_reasoner::classify(&ontology).map_err(reason_error_to_py)?;
    let classes = classification.classes();
    let unsat: std::collections::HashSet<&str> =
        classification.unsatisfiable_classes().into_iter().collect();
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
pub(crate) fn materialize_inferred_class_assertions(path: &str) -> PyResult<Vec<(String, String)>> {
    let ontology = load::load_path(path)?;
    let realization = owl_dl_reasoner::realize(&ontology).map_err(reason_error_to_py)?;
    let mut out = Vec::new();
    for ind in realization.individuals() {
        for c in realization.most_specific_types(ind) {
            out.push((c.clone(), ind.clone()));
        }
    }
    Ok(out)
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(materialize_inferred_subclass_axioms, m)?)?;
    m.add_function(wrap_pyfunction!(materialize_inferred_class_assertions, m)?)?;
    Ok(())
}
