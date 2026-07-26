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
#[pyfunction]
pub(crate) fn realize(path: &str) -> PyResult<HashMap<String, Vec<String>>> {
    let ontology = load::load_path(path)?;
    let rs_realization = owl_dl_reasoner::realize(&ontology).map_err(reason_error_to_py)?;
    Ok(realization_to_dict(&rs_realization))
}

fn realization_to_dict(realization: &owl_dl_reasoner::Realization) -> HashMap<String, Vec<String>> {
    realization
        .individuals()
        .iter()
        .map(|ind| (ind.clone(), realization.most_specific_types(ind).to_vec()))
        .collect()
}

/// Entailed disjoint named-class pairs `(c, d)` — `C ⊓ D` is proven
/// unsatisfiable — plus a completeness flag. Bounded by a 1s per-pair
/// deadline. The Python-level wrapper (`__init__.py`) unpacks the tuple,
/// warns on `incomplete`, and returns just the pair list (see
/// `IncompleteQueryWarning`).
#[pyfunction]
pub(crate) fn disjoint_classes(path: &str) -> PyResult<(Vec<(String, String)>, bool)> {
    let ontology = load::load_path(path)?;
    owl_dl_reasoner::disjoint_classes(&ontology, Some(std::time::Duration::from_secs(1)))
        .map(|d| (d.pairs().to_vec(), d.incomplete()))
        .map_err(reason_error_to_py)
}

/// Told-disjoint object property pairs `(a, b)`.
#[pyfunction]
pub(crate) fn disjoint_object_properties(path: &str) -> PyResult<Vec<(String, String)>> {
    let ontology = load::load_path(path)?;
    owl_dl_reasoner::disjoint_object_properties(&ontology).map_err(reason_error_to_py)
}

/// Told-disjoint data property pairs `(a, b)`.
#[pyfunction]
pub(crate) fn disjoint_data_properties(path: &str) -> PyResult<Vec<(String, String)>> {
    let ontology = load::load_path(path)?;
    owl_dl_reasoner::disjoint_data_properties(&ontology).map_err(reason_error_to_py)
}

/// Inferred object property hierarchy: `(equivalent_groups, direct_subsumptions)`.
#[pyfunction]
#[allow(clippy::type_complexity)]
pub(crate) fn object_property_hierarchy(
    path: &str,
) -> PyResult<(Vec<Vec<String>>, Vec<(String, String)>)> {
    let o = load::load_path(path)?;
    let c = owl_dl_reasoner::classify_object_property_hierarchy(&o).map_err(reason_error_to_py)?;
    Ok((
        c.equivalent_groups().to_vec(),
        c.direct_subsumptions().to_vec(),
    ))
}

/// Inferred data property hierarchy: `(equivalent_groups, direct_subsumptions)`.
#[pyfunction]
#[allow(clippy::type_complexity)]
pub(crate) fn data_property_hierarchy(
    path: &str,
) -> PyResult<(Vec<Vec<String>>, Vec<(String, String)>)> {
    let o = load::load_path(path)?;
    let c = owl_dl_reasoner::classify_data_property_hierarchy(&o).map_err(reason_error_to_py)?;
    Ok((
        c.equivalent_groups().to_vec(),
        c.direct_subsumptions().to_vec(),
    ))
}

/// Entailed same-individual equivalence groups (asserted + functional-forced
/// + entailed), plus a completeness flag. Bounded by a 1s per-pair deadline.
/// See `disjoint_classes` for the tuple/wrapper convention.
#[pyfunction]
pub(crate) fn same_individuals(path: &str) -> PyResult<(Vec<Vec<String>>, bool)> {
    let o = load::load_path(path)?;
    owl_dl_reasoner::same_individuals(&o, Some(std::time::Duration::from_secs(1)))
        .map(|s| (s.groups().to_vec(), s.incomplete()))
        .map_err(reason_error_to_py)
}

/// Entailed different-individual pairs `(a, b)` — `{a} ⊓ {b}` is proven
/// unsatisfiable — plus a completeness flag. Bounded by a 1s per-pair
/// deadline. See `disjoint_classes` for the tuple/wrapper convention.
#[pyfunction]
pub(crate) fn different_individuals(path: &str) -> PyResult<(Vec<(String, String)>, bool)> {
    let o = load::load_path(path)?;
    owl_dl_reasoner::different_individuals(&o, Some(std::time::Duration::from_secs(1)))
        .map(|d| (d.pairs().to_vec(), d.incomplete()))
        .map_err(reason_error_to_py)
}

/// Inferred object property values `(subject, property, object)` over named
/// individuals, plus a completeness flag. Bounded by a 1s per-pair deadline.
/// See `disjoint_classes` for the tuple/wrapper convention.
#[pyfunction]
#[allow(clippy::type_complexity)]
pub(crate) fn object_property_values(
    path: &str,
) -> PyResult<(Vec<(String, String, String)>, bool)> {
    let o = load::load_path(path)?;
    owl_dl_reasoner::inferred_object_property_values(&o, Some(std::time::Duration::from_secs(1)))
        .map(|v| (v.triples().to_vec(), v.incomplete()))
        .map_err(reason_error_to_py)
}

/// Inferred data property values `(subject, property, lexical, datatype)`
/// over named individuals.
#[pyfunction]
#[allow(clippy::type_complexity)]
pub(crate) fn data_property_values(path: &str) -> PyResult<Vec<(String, String, String, String)>> {
    let o = load::load_path(path)?;
    owl_dl_reasoner::inferred_data_property_values(&o)
        .map(|v| v.quads().to_vec())
        .map_err(reason_error_to_py)
}

/// Kinds and counts of axioms conversion could not represent (a sound
/// under-approximation) — conversion never aborts on an unsupported
/// construct, it records the drop and continues, so `classify` /
/// `is_consistent` / `realize` still succeed on such inputs.
#[pyfunction]
pub(crate) fn dropped_axioms(path: &str) -> PyResult<std::collections::BTreeMap<String, u64>> {
    let o = load::load_path(path)?;
    owl_dl_reasoner::dropped_axioms(&o)
        .map(|d| d.by_kind().clone())
        .map_err(reason_error_to_py)
}

/// Parse a Manchester-syntax class expression against the ontology's own
/// prefix map (so e.g. `:A` resolves via the file's `Prefix(:=...)`).
fn parse_ce(
    ce: &str,
    pm: &horned_owl::curie::PrefixMapping,
    build: &horned_owl::model::Build<horned_owl::model::RcStr>,
) -> PyResult<horned_owl::model::ClassExpression<horned_owl::model::RcStr>> {
    horned_owl::io::omn::reader::parse_class_expression(ce, pm, build)
        .map_err(|e| pyo3::exceptions::PyValueError::new_err(format!("class expression: {e}")))
}

/// Is `ce` satisfiable w.r.t. the ontology at `path`? Plus a completeness
/// flag. See `disjoint_classes` for the tuple/wrapper convention.
#[pyfunction]
pub(crate) fn class_expression_satisfiable(path: &str, ce: &str) -> PyResult<(bool, bool)> {
    let (o, pm) = load::load_path_with_pm(path)?;
    let build: horned_owl::model::Build<horned_owl::model::RcStr> = horned_owl::model::Build::new();
    let ce = parse_ce(ce, &pm, &build)?;
    let v = owl_dl_reasoner::class_expression_satisfiable(&o, &ce).map_err(reason_error_to_py)?;
    Ok((v.holds(), v.incomplete()))
}

/// Is `sub_ce ⊑ sup_ce` entailed w.r.t. the ontology at `path`? Plus a
/// completeness flag. See `disjoint_classes` for the tuple/wrapper convention.
#[pyfunction]
pub(crate) fn class_expression_entailed_subclass(
    path: &str,
    sub_ce: &str,
    sup_ce: &str,
) -> PyResult<(bool, bool)> {
    let (o, pm) = load::load_path_with_pm(path)?;
    let build: horned_owl::model::Build<horned_owl::model::RcStr> = horned_owl::model::Build::new();
    let sub_ce = parse_ce(sub_ce, &pm, &build)?;
    let sup_ce = parse_ce(sup_ce, &pm, &build)?;
    let v = owl_dl_reasoner::class_expression_entailed_subclass(&o, &sub_ce, &sup_ce)
        .map_err(reason_error_to_py)?;
    Ok((v.holds(), v.incomplete()))
}

/// Named individuals provably in `ce` w.r.t. the ontology at `path`, plus a
/// completeness flag. See `disjoint_classes` for the tuple/wrapper convention.
#[pyfunction]
pub(crate) fn class_expression_instances(path: &str, ce: &str) -> PyResult<(Vec<String>, bool)> {
    let (o, pm) = load::load_path_with_pm(path)?;
    let build: horned_owl::model::Build<horned_owl::model::RcStr> = horned_owl::model::Build::new();
    let ce = parse_ce(ce, &pm, &build)?;
    let r = owl_dl_reasoner::class_expression_instances(&o, &ce).map_err(reason_error_to_py)?;
    Ok((r.individuals().to_vec(), r.incomplete()))
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(is_consistent, m)?)?;
    m.add_function(wrap_pyfunction!(is_class_satisfiable, m)?)?;
    m.add_function(wrap_pyfunction!(is_subclass_of, m)?)?;
    m.add_function(wrap_pyfunction!(is_instance_of, m)?)?;
    m.add_function(wrap_pyfunction!(instances_of, m)?)?;
    m.add_function(wrap_pyfunction!(realize, m)?)?;
    m.add_function(wrap_pyfunction!(disjoint_classes, m)?)?;
    m.add_function(wrap_pyfunction!(disjoint_object_properties, m)?)?;
    m.add_function(wrap_pyfunction!(disjoint_data_properties, m)?)?;
    m.add_function(wrap_pyfunction!(object_property_hierarchy, m)?)?;
    m.add_function(wrap_pyfunction!(data_property_hierarchy, m)?)?;
    m.add_function(wrap_pyfunction!(same_individuals, m)?)?;
    m.add_function(wrap_pyfunction!(different_individuals, m)?)?;
    m.add_function(wrap_pyfunction!(object_property_values, m)?)?;
    m.add_function(wrap_pyfunction!(data_property_values, m)?)?;
    m.add_function(wrap_pyfunction!(class_expression_satisfiable, m)?)?;
    m.add_function(wrap_pyfunction!(class_expression_entailed_subclass, m)?)?;
    m.add_function(wrap_pyfunction!(class_expression_instances, m)?)?;
    m.add_function(wrap_pyfunction!(dropped_axioms, m)?)?;
    Ok(())
}
