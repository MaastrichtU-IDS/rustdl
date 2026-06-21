//! Explanation / justification helpers — the minimal responsible-axiom set(s)
//! for an entailment ("why does this hold?"), via the reasoner's black-box
//! justification search (`QuickXplain` for one, Reiter HST for all). Reuses
//! `owl_dl_reasoner::justify`; no engine changes.

use horned_owl::curie::PrefixMapping;
use horned_owl::io::omn::AsManchester;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::justify::{Entailment, find_all_justifications, find_one_justification};
use pyo3::prelude::*;

use crate::errors::reason_error_to_py;
use crate::load;

/// Run the justification search for `q` and render each justification as a list
/// of Manchester axiom strings. `all = false` → at most one (`QuickXplain`);
/// `all = true` → up to `max` (Reiter Hitting-Set Tree). An empty outer list
/// means `q` is not entailed (nothing to justify).
fn justify(
    onto: &SetOntology<RcStr>,
    q: &Entailment,
    all: bool,
    max: usize,
) -> PyResult<Vec<Vec<String>>> {
    let pm = PrefixMapping::default();
    let justs = if all {
        find_all_justifications(onto, q, max).map_err(reason_error_to_py)?
    } else {
        find_one_justification(onto, q)
            .map_err(reason_error_to_py)?
            .into_iter()
            .collect()
    };
    Ok(justs
        .into_iter()
        .map(|j| {
            j.axioms
                .iter()
                .map(|ax| ax.as_manchester_with_prefixes(&pm).to_string())
                .collect()
        })
        .collect())
}

/// Explain an entailed `SubClassOf(sub, sup)`. Returns a list of justifications,
/// each a list of Manchester-rendered axiom strings. Empty list ⇒ not entailed.
#[pyfunction]
#[pyo3(signature = (path, sub, sup, *, all=false, max=10))]
pub(crate) fn explain(
    path: &str,
    sub: &str,
    sup: &str,
    all: bool,
    max: usize,
) -> PyResult<Vec<Vec<String>>> {
    let onto = load::load_path(path)?;
    justify(
        &onto,
        &Entailment::SubClassOf {
            sub: sub.to_string(),
            sup: sup.to_string(),
        },
        all,
        max,
    )
}

/// Explain why `class` is unsatisfiable (entailed equivalent to `owl:Nothing`).
#[pyfunction]
#[pyo3(signature = (path, class, *, all=false, max=10))]
pub(crate) fn explain_unsatisfiable(
    path: &str,
    class: &str,
    all: bool,
    max: usize,
) -> PyResult<Vec<Vec<String>>> {
    let onto = load::load_path(path)?;
    justify(
        &onto,
        &Entailment::Unsatisfiable {
            class: class.to_string(),
        },
        all,
        max,
    )
}

/// Explain why the ontology is inconsistent. Empty list ⇒ it is consistent.
#[pyfunction]
#[pyo3(signature = (path, *, all=false, max=10))]
pub(crate) fn explain_inconsistency(
    path: &str,
    all: bool,
    max: usize,
) -> PyResult<Vec<Vec<String>>> {
    let onto = load::load_path(path)?;
    justify(&onto, &Entailment::Inconsistent, all, max)
}

/// Explain an entailed `ClassAssertion(class, individual)`.
#[pyfunction]
#[pyo3(signature = (path, individual, class, *, all=false, max=10))]
pub(crate) fn explain_instance(
    path: &str,
    individual: &str,
    class: &str,
    all: bool,
    max: usize,
) -> PyResult<Vec<Vec<String>>> {
    let onto = load::load_path(path)?;
    justify(
        &onto,
        &Entailment::InstanceOf {
            individual: individual.to_string(),
            class: class.to_string(),
        },
        all,
        max,
    )
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(explain, m)?)?;
    m.add_function(wrap_pyfunction!(explain_unsatisfiable, m)?)?;
    m.add_function(wrap_pyfunction!(explain_inconsistency, m)?)?;
    m.add_function(wrap_pyfunction!(explain_instance, m)?)?;
    Ok(())
}
