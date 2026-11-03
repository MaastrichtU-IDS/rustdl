//! Top-level query bindings: consistency, satisfiability, subsumption,
//! instance checks, realization.

use std::collections::HashMap;

use pyo3::prelude::*;

use crate::errors::reason_error_to_py;
use crate::load;

/// `(consistent, incomplete, relies_on_trusted_consistency)` for the ontology at
/// `path`. `incomplete` is set when "consistent" is a give-up rather than a
/// verdict (the `QueryStats` contract, #363 A2); the third flag when it rests
/// only on the consistency wedge's unverified `Sat` (#392). The Python wrapper
/// (`__init__.py`) warns on each and returns the bool.
#[pyfunction]
pub(crate) fn is_consistent(path: &str) -> PyResult<(bool, bool, bool)> {
    let ontology = load::load_path(path)?;
    let (verdict, stats) =
        owl_dl_reasoner::is_consistent_with_stats(&ontology).map_err(reason_error_to_py)?;
    Ok((
        verdict,
        verdict && stats.incomplete,
        stats.relies_on_trusted_consistency,
    ))
}

/// `(satisfiable, incomplete, relies_on_trusted_consistency,
/// consistency_undetermined)`: `incomplete` means the tableau probe gave up (node
/// cap) and "satisfiable" is the sound direction of no verdict; the third flag is
/// #401's `QueryStats::relies_on_trusted_consistency`, the fourth #428's
/// `QueryStats::consistency_undetermined`.
#[pyfunction]
pub(crate) fn is_class_satisfiable(
    path: &str,
    class_iri: &str,
) -> PyResult<(bool, bool, bool, bool)> {
    let ontology = load::load_path(path)?;
    let _cap = owl_dl_reasoner::single_query_node_cap();
    let (sat, stats) = owl_dl_reasoner::is_class_satisfiable_with_stats(&ontology, class_iri)
        .map_err(reason_error_to_py)?;
    Ok((
        sat,
        stats.incomplete,
        stats.relies_on_trusted_consistency,
        stats.consistency_undetermined,
    ))
}

/// `(subsumed, incomplete, relies_on_trusted_consistency,
/// consistency_undetermined)`: `incomplete` means the probe gave up and "not
/// subsumed" is the sound direction of no verdict; the third flag is #401's
/// `QueryStats::relies_on_trusted_consistency`, the fourth #428's
/// `QueryStats::consistency_undetermined`.
#[pyfunction]
pub(crate) fn is_subclass_of(
    path: &str,
    sub: &str,
    sup: &str,
) -> PyResult<(bool, bool, bool, bool)> {
    let ontology = load::load_path(path)?;
    let _cap = owl_dl_reasoner::single_query_node_cap();
    let (subsumed, stats) = owl_dl_reasoner::is_subclass_of_with_stats(&ontology, sub, sup)
        .map_err(reason_error_to_py)?;
    Ok((
        subsumed,
        stats.incomplete,
        stats.relies_on_trusted_consistency,
        stats.consistency_undetermined,
    ))
}

/// `(is_instance, incomplete, consistency_undetermined)`: `incomplete` means the
/// probe was cut (deadline, node cap or depth limit) and "no" is the sound
/// direction; `consistency_undetermined` (#369) means a "yes" may be the vacuous
/// answer of an inconsistent KB (`InstanceFlags`).
#[pyfunction]
pub(crate) fn is_instance_of(
    path: &str,
    class_iri: &str,
    individual_iri: &str,
) -> PyResult<(bool, bool, bool)> {
    let ontology = load::load_path(path)?;
    let _cap = owl_dl_reasoner::single_query_node_cap();
    let (holds, flags) =
        owl_dl_reasoner::is_instance_of_reporting(&ontology, class_iri, individual_iri)
            .map_err(reason_error_to_py)?;
    Ok((holds, flags.incomplete, flags.consistency_undetermined))
}

/// `(individuals, incomplete, consistency_undetermined)`: named individuals
/// entailed to be instances of `class_iri`; `incomplete` means at least one probe
/// was cut; `consistency_undetermined` as in `is_instance_of` (#369).
#[pyfunction]
pub(crate) fn instances_of(path: &str, class_iri: &str) -> PyResult<(Vec<String>, bool, bool)> {
    let ontology = load::load_path(path)?;
    let _cap = owl_dl_reasoner::single_query_node_cap();
    let (members, flags) = owl_dl_reasoner::instances_of_reporting(&ontology, class_iri)
        .map_err(reason_error_to_py)?;
    Ok((members, flags.incomplete, flags.consistency_undetermined))
}

/// `(types, incomplete, direct_types_unverified, witness_prune_may_drop)`: each
/// named individual mapped to its most-specific entailed types; `incomplete`
/// means at least one instance probe was cut (`Realization::incomplete`);
/// `direct_types_unverified` means a most-specific set may list a superclass of
/// another listed type (`Realization::direct_types_unverified`, #388);
/// `witness_prune_may_drop` means the pseudo-model prune ran on a witness with a
/// known reason to lack an entailed type (`Realization::witness_prune_may_drop`,
/// #387).
#[pyfunction]
#[allow(clippy::type_complexity)]
pub(crate) fn realize(path: &str) -> PyResult<(HashMap<String, Vec<String>>, bool, bool, bool)> {
    let ontology = load::load_path(path)?;
    let rs_realization = owl_dl_reasoner::realize(&ontology).map_err(reason_error_to_py)?;
    Ok((
        realization_to_dict(&rs_realization),
        rs_realization.incomplete(),
        rs_realization.direct_types_unverified(),
        rs_realization.witness_prune_may_drop(),
    ))
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

/// The #389/#390 structural gaps of `onto`; see
/// `owl_dl_reasoner::property_structural_gaps`.
pub(crate) fn gaps<A: horned_owl::model::ForIRI>(
    onto: &horned_owl::ontology::set::SetOntology<A>,
) -> PyResult<owl_dl_reasoner::PropertyStructuralGaps> {
    owl_dl_reasoner::property_structural_gaps(onto).map_err(reason_error_to_py)
}

/// Told-disjoint object property pairs `(a, b)`, plus the structural gaps (#389)
/// and #369's `consistency_undetermined`. The Python wrapper unpacks the tuple
/// and emits `StructuralGapWarning` / `ConsistencyUndeterminedWarning`.
#[pyfunction]
#[allow(clippy::type_complexity)]
pub(crate) fn disjoint_object_properties(
    path: &str,
) -> PyResult<(Vec<(String, String)>, Vec<String>, bool)> {
    let ontology = load::load_path(path)?;
    let (pairs, undetermined) = owl_dl_reasoner::disjoint_object_properties_reporting(&ontology)
        .map_err(reason_error_to_py)?;
    Ok((
        pairs,
        gaps(&ontology)?.object_disjoint().to_vec(),
        undetermined,
    ))
}

/// Told-disjoint data property pairs `(a, b)`, plus the structural gaps (#389)
/// and #369's `consistency_undetermined`.
#[pyfunction]
#[allow(clippy::type_complexity)]
pub(crate) fn disjoint_data_properties(
    path: &str,
) -> PyResult<(Vec<(String, String)>, Vec<String>, bool)> {
    let ontology = load::load_path(path)?;
    let (pairs, undetermined) = owl_dl_reasoner::disjoint_data_properties_reporting(&ontology)
        .map_err(reason_error_to_py)?;
    Ok((
        pairs,
        gaps(&ontology)?.data_disjoint().to_vec(),
        undetermined,
    ))
}

/// Inferred object property hierarchy: `(equivalent_groups, direct_subsumptions,
/// structural_gaps)` (#389).
#[pyfunction]
#[allow(clippy::type_complexity)]
pub(crate) fn object_property_hierarchy(
    path: &str,
) -> PyResult<(Vec<Vec<String>>, Vec<(String, String)>, Vec<String>)> {
    let o = load::load_path(path)?;
    let h = owl_dl_reasoner::classify_property_hierarchies(&o).map_err(reason_error_to_py)?;
    Ok((
        h.object.equivalent_groups().to_vec(),
        h.object.direct_subsumptions().to_vec(),
        h.gaps.object_hierarchy().to_vec(),
    ))
}

/// Inferred data property hierarchy: `(equivalent_groups, direct_subsumptions,
/// structural_gaps)` (#389).
#[pyfunction]
#[allow(clippy::type_complexity)]
pub(crate) fn data_property_hierarchy(
    path: &str,
) -> PyResult<(Vec<Vec<String>>, Vec<(String, String)>, Vec<String>)> {
    let o = load::load_path(path)?;
    let h = owl_dl_reasoner::classify_property_hierarchies(&o).map_err(reason_error_to_py)?;
    Ok((
        h.data.equivalent_groups().to_vec(),
        h.data.direct_subsumptions().to_vec(),
        h.gaps.data_hierarchy().to_vec(),
    ))
}

/// Entailed same-individual equivalence groups (asserted + functional-forced
/// + entailed), plus a completeness flag. Bounded by a 1s per-pair deadline.
/// See `disjoint_classes` for the tuple/wrapper convention.
#[pyfunction]
pub(crate) fn same_individuals(path: &str) -> PyResult<(Vec<Vec<String>>, bool, bool)> {
    let o = load::load_path(path)?;
    owl_dl_reasoner::same_individuals(&o, Some(std::time::Duration::from_secs(1)))
        .map(|s| {
            (
                s.groups().to_vec(),
                s.incomplete(),
                s.consistency_undetermined(),
            )
        })
        .map_err(reason_error_to_py)
}

/// Entailed different-individual pairs `(a, b)` — `{a} ⊓ {b}` is proven
/// unsatisfiable — plus a completeness flag. Bounded by a 1s per-pair
/// deadline. See `disjoint_classes` for the tuple/wrapper convention.
#[pyfunction]
#[allow(clippy::type_complexity)]
pub(crate) fn different_individuals(path: &str) -> PyResult<(Vec<(String, String)>, bool, bool)> {
    let o = load::load_path(path)?;
    owl_dl_reasoner::different_individuals(&o, Some(std::time::Duration::from_secs(1)))
        .map(|d| {
            (
                d.pairs().to_vec(),
                d.incomplete(),
                d.consistency_undetermined(),
            )
        })
        .map_err(reason_error_to_py)
}

/// Inferred object property values `(subject, property, object)` over named
/// individuals, plus a completeness flag. Bounded by a 1s per-pair deadline.
/// See `disjoint_classes` for the tuple/wrapper convention.
#[pyfunction]
#[allow(clippy::type_complexity)]
pub(crate) fn object_property_values(
    path: &str,
) -> PyResult<(Vec<(String, String, String)>, bool, bool)> {
    let o = load::load_path(path)?;
    owl_dl_reasoner::inferred_object_property_values(&o, Some(std::time::Duration::from_secs(1)))
        .map(|v| {
            (
                v.triples().to_vec(),
                v.incomplete(),
                v.consistency_undetermined(),
            )
        })
        .map_err(reason_error_to_py)
}

/// Inferred data property values `(subject, property, lexical, datatype, lang)`
/// over named individuals, plus the structural gaps (#390).
#[pyfunction]
#[allow(clippy::type_complexity)]
pub(crate) fn data_property_values(
    path: &str,
) -> PyResult<(
    Vec<(String, String, String, String, String)>,
    Vec<String>,
    bool,
)> {
    let o = load::load_path(path)?;
    let v = owl_dl_reasoner::inferred_data_property_values(&o).map_err(reason_error_to_py)?;
    Ok((
        v.quints().to_vec(),
        gaps(&o)?.data_values().to_vec(),
        v.consistency_undetermined(),
    ))
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

/// `(satisfiable, incomplete, relies_on_trusted_consistency,
/// consistency_undetermined)` for the class expression `ce` (the last is #369's,
/// set on an "unsatisfiable" whose KB consistency stayed undetermined): `incomplete` means the probe gave up (node cap) and
/// "satisfiable" is the sound direction (#363 A2); an answer off pure EL is not
/// `incomplete`. Runs under the single-query node cap, like
/// `is_class_satisfiable`.
#[pyfunction]
pub(crate) fn class_expression_satisfiable(
    path: &str,
    ce: &str,
) -> PyResult<(bool, bool, bool, bool)> {
    let (o, pm) = load::load_path_with_pm(path)?;
    let _cap = owl_dl_reasoner::single_query_node_cap();
    let build: horned_owl::model::Build<horned_owl::model::RcStr> = horned_owl::model::Build::new();
    let ce = parse_ce(ce, &pm, &build)?;
    let v = owl_dl_reasoner::class_expression_satisfiable(&o, &ce).map_err(reason_error_to_py)?;
    Ok((
        v.holds(),
        v.incomplete(),
        v.relies_on_trusted_consistency(),
        v.consistency_undetermined(),
    ))
}

/// `(entailed, incomplete, relies_on_trusted_consistency,
/// consistency_undetermined)` for `sub_ce ⊑ sup_ce` (the last as in
/// `class_expression_satisfiable`): `incomplete` means the probe gave up and "not entailed"
/// is the sound direction; an answer off pure EL is not `incomplete`. Runs
/// under the single-query node cap, like `is_subclass_of`.
#[pyfunction]
pub(crate) fn class_expression_entailed_subclass(
    path: &str,
    sub_ce: &str,
    sup_ce: &str,
) -> PyResult<(bool, bool, bool, bool)> {
    let (o, pm) = load::load_path_with_pm(path)?;
    let _cap = owl_dl_reasoner::single_query_node_cap();
    let build: horned_owl::model::Build<horned_owl::model::RcStr> = horned_owl::model::Build::new();
    let sub_ce = parse_ce(sub_ce, &pm, &build)?;
    let sup_ce = parse_ce(sup_ce, &pm, &build)?;
    let v = owl_dl_reasoner::class_expression_entailed_subclass(&o, &sub_ce, &sup_ce)
        .map_err(reason_error_to_py)?;
    Ok((
        v.holds(),
        v.incomplete(),
        v.relies_on_trusted_consistency(),
        v.consistency_undetermined(),
    ))
}

/// `(individuals, incomplete)`: named individuals provably in `ce`;
/// `incomplete` means at least one instance probe was cut, so members may be
/// missing (those returned are sound). Runs under the single-query node cap,
/// like `instances_of`.
#[pyfunction]
pub(crate) fn class_expression_instances(
    path: &str,
    ce: &str,
) -> PyResult<(Vec<String>, bool, bool)> {
    let (o, pm) = load::load_path_with_pm(path)?;
    let _cap = owl_dl_reasoner::single_query_node_cap();
    let build: horned_owl::model::Build<horned_owl::model::RcStr> = horned_owl::model::Build::new();
    let ce = parse_ce(ce, &pm, &build)?;
    let r = owl_dl_reasoner::class_expression_instances(&o, &ce).map_err(reason_error_to_py)?;
    Ok((
        r.individuals().to_vec(),
        r.incomplete(),
        r.consistency_undetermined(),
    ))
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
