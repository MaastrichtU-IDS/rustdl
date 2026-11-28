//! Python bindings for the explanation/debugging suite (justify, diagnose, repair).
//! String/tuple forms; axioms rendered as Manchester with full IRIs.

use horned_owl::curie::PrefixMapping;
use horned_owl::io::omn::AsManchester;
use horned_owl::model::{Component, RcStr};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::PyDict;

use crate::errors::reason_error_to_py;
use crate::load;

fn render(ax: &Component<RcStr>) -> String {
    ax.as_manchester_with_prefixes(&PrefixMapping::default())
        .to_string()
}

/// Every logical axiom of the ontology at `path`, rendered as Manchester
/// syntax strings (declarations, imports, and ontology annotations are
/// skipped as non-logical noise). Same renderer `justify` uses.
#[pyfunction]
pub(crate) fn render_manchester(path: &str) -> PyResult<Vec<String>> {
    let onto = load::load_path(path)?;
    Ok(onto
        .iter()
        .map(|ac| &ac.component)
        .filter(|c| is_logical_axiom(c))
        .map(render)
        .collect())
}

fn is_logical_axiom(c: &Component<RcStr>) -> bool {
    use Component::{
        AnnotationAssertion, DeclareAnnotationProperty, DeclareClass, DeclareDataProperty,
        DeclareDatatype, DeclareNamedIndividual, DeclareObjectProperty, DocIRI, Import,
        OntologyAnnotation, OntologyID,
    };
    // Exclude declarations, imports, and ontology-level metadata; keep the
    // class/property/individual axioms that carry entailment meaning.
    !matches!(
        c,
        OntologyID(_)
            | DocIRI(_)
            | Import(_)
            | OntologyAnnotation(_)
            | DeclareClass(_)
            | DeclareObjectProperty(_)
            | DeclareAnnotationProperty(_)
            | DeclareDataProperty(_)
            | DeclareNamedIndividual(_)
            | DeclareDatatype(_)
            | AnnotationAssertion(_)
    )
}

/// #625: a justify outcome as a dict, so no position can be transposed. `cap` is
/// `None` for a single-justification query, and `Some(max)` for `justify_all`. Keys:
/// - `justifications`: each justification's axioms as Manchester strings;
/// - `minimal`: `Justification::minimal_guaranteed`, one per justification;
/// - `incomplete` (#396), `relies_on_trusted_consistency` (#429), `depth_capped` (#510),
///   `saturation_oracle` (#584): the outcome's flags;
/// - `in_fragment`: the ontology is pure EL or Horn (true when none was found);
/// - `enumeration_complete`: no justification is known to be missing. For a single query:
///   false only when none was found and the search gave up. For `justify_all`: nothing
///   gave up, the saturation-only oracle did not run, every justification is minimal (a
///   non-minimal one marks a search whose "not entailed" may have pruned a branch, as
///   `repair` reads it), and fewer than `max` were found. The search runs with `max`
///   itself (not the CLI's `max + 1` probe, whose extra search could set `incomplete` or
///   the trust flag for a justification it then discards), so exactly `max` found reads
///   as not known to be complete.
fn outcome_dict<'py>(
    py: Python<'py>,
    outcome: &owl_dl_reasoner::justify::JustifyOutcome<RcStr>,
    cap: Option<usize>,
) -> PyResult<Bound<'py, PyDict>> {
    let kept = &outcome.justifications;
    let js: Vec<Vec<String>> = kept
        .iter()
        .map(|j| j.axioms.iter().map(render).collect())
        .collect();
    let minimal: Vec<bool> = kept.iter().map(|j| j.minimal_guaranteed).collect();
    let in_fragment = outcome.justifications.first().is_none_or(|j| {
        matches!(
            j.fragment,
            owl_dl_reasoner::FragmentClassification::PureEl
                | owl_dl_reasoner::FragmentClassification::Horn
        )
    });
    let enumeration_complete = match cap {
        None => !kept.is_empty() || !outcome.incomplete,
        Some(max) => {
            !outcome.incomplete
                && !outcome.saturation_oracle
                && minimal.iter().all(|m| *m)
                && kept.len() < max
        }
    };
    let d = PyDict::new(py);
    d.set_item("justifications", js)?;
    d.set_item("minimal", minimal)?;
    d.set_item("incomplete", outcome.incomplete)?;
    d.set_item(
        "relies_on_trusted_consistency",
        outcome.relies_on_trusted_consistency,
    )?;
    // #510: an instance check's give-up was the decision-depth limit.
    d.set_item("depth_capped", outcome.depth_capped)?;
    d.set_item("saturation_oracle", outcome.saturation_oracle)?;
    d.set_item("in_fragment", in_fragment)?;
    d.set_item("enumeration_complete", enumeration_complete)?;
    Ok(d)
}

/// One justification for a query (CLI-style tokens, e.g. `["subclass", sub, sup]`,
/// `["unsat", c]`, `["inconsistent"]`) as [`outcome_dict`]: at most one entry, none if
/// not entailed or unknown. The `rustdl.justify` / `justify_result` wrappers warn.
#[pyfunction]
#[allow(clippy::needless_pass_by_value)]
pub(crate) fn justify<'py>(
    py: Python<'py>,
    path: &str,
    query: Vec<String>,
) -> PyResult<Bound<'py, PyDict>> {
    let onto = load::load_path_noted(path)?;
    let q = owl_dl_reasoner::justify::parse_query(&query).map_err(PyValueError::new_err)?;
    let outcome = owl_dl_reasoner::justify::find_one_justification_reporting(&onto, &q)
        .map_err(reason_error_to_py)?;
    outcome_dict(py, &outcome, None)
}

/// Up to `max` justifications as [`outcome_dict`] (#625).
#[pyfunction]
#[pyo3(signature = (path, query, max = 10))]
#[allow(clippy::needless_pass_by_value)]
pub(crate) fn justify_all<'py>(
    py: Python<'py>,
    path: &str,
    query: Vec<String>,
    max: usize,
) -> PyResult<Bound<'py, PyDict>> {
    let onto = load::load_path_noted(path)?;
    let q = owl_dl_reasoner::justify::parse_query(&query).map_err(PyValueError::new_err)?;
    let outcome = owl_dl_reasoner::justify::find_all_justifications_reporting(&onto, &q, max)
        .map_err(reason_error_to_py)?;
    outcome_dict(py, &outcome, Some(max))
}

/// Root/derived unsatisfiability partition:
/// `(consistent, roots, [(derived_iri, [root_iri, ...]), ...], incomplete,
/// relies_on_trusted_consistency)` (the last is #429's).
/// `incomplete` (#396): the consistency check of an all-classes-unsatisfiable
/// ontology gave up (`consistent` is not witnessed), or a class's
/// unsatisfiability check hit the node cap (unsatisfiable classes may be
/// missing).
#[pyfunction]
#[allow(clippy::type_complexity)]
pub(crate) fn diagnose(
    path: &str,
) -> PyResult<(bool, Vec<String>, Vec<(String, Vec<String>)>, bool, bool)> {
    let onto = load::load_path_noted(path)?;
    let d = owl_dl_reasoner::diagnose(&onto).map_err(reason_error_to_py)?;
    let derived = d.derived.into_iter().map(|dc| (dc.iri, dc.roots)).collect();
    Ok((
        d.consistent,
        d.roots,
        derived,
        d.incomplete,
        d.relies_on_trusted_consistency,
    ))
}

/// Minimal repairs for a query: each is a list of Manchester axioms to remove.
/// Paired with `incomplete`: a reasoner check gave up (#396), so an empty list
/// may mean "unknown", and repairs may be missing; every repair returned is
/// still verified.
#[pyfunction]
#[pyo3(signature = (path, query, max = 10))]
#[allow(clippy::needless_pass_by_value)]
pub(crate) fn repair(
    path: &str,
    query: Vec<String>,
    max: usize,
) -> PyResult<(Vec<Vec<String>>, bool, bool, bool)> {
    let onto = load::load_path_noted(path)?;
    let q = owl_dl_reasoner::justify::parse_query(&query).map_err(PyValueError::new_err)?;
    let r = owl_dl_reasoner::find_repairs(&onto, &q, max).map_err(reason_error_to_py)?;
    let repairs = r
        .repairs
        .into_iter()
        .map(|rep| rep.remove.iter().map(render).collect())
        .collect();
    Ok((
        repairs,
        r.incomplete,
        r.relies_on_trusted_consistency,
        r.depth_capped,
    ))
}

/// A loaded ontology with the per-ontology justification state precomputed once
/// (parse + logical-axiom split + fragment classification). Reuse it to justify
/// many queries without re-doing that full-ontology work each call — a large
/// win on big ontologies. Unsendable: use it from the thread that created it.
#[pyclass(name = "PreparedOntology", module = "rustdl", unsendable)]
pub(crate) struct PyPreparedOntology {
    inner: owl_dl_reasoner::justify::PreparedJustifier<RcStr>,
    /// #543, computed once at prepare time; the wrappers warn `UniversalRoleWarning`.
    universal_role_ordinary: bool,
    /// #589, computed once at prepare time; the wrappers warn `DataCaseSplitWarning`.
    data_case_split_risk: bool,
}

/// `(universal_role_ordinary, data_case_split_risk)` for a prepared ontology: #543's
/// and #589's flags, from one conversion at prepare time (#619: no byte prefilter; a
/// prefix that cuts inside the name hid the use from it). A failed conversion reads
/// as neither flag.
fn prepared_flags(onto: &horned_owl::ontology::set::SetOntology<RcStr>) -> (bool, bool) {
    owl_dl_reasoner::conversion_flags(onto)
        .map(|f| (f.universal_role_ordinary, f.data_case_split_risk))
        .unwrap_or_default()
}

#[pymethods]
impl PyPreparedOntology {
    /// #543: the ontology uses a universal role where rustdl's ordinary-property
    /// reading can miss entailments; a "not entailed" may be wrong.
    #[getter]
    fn universal_role_ordinary(&self) -> bool {
        self.universal_role_ordinary
    }

    /// #589: some data range may need reasoning by cases over its values, which
    /// rustdl does not do; a "not entailed" may be wrong.
    #[getter]
    fn data_case_split_risk(&self) -> bool {
        self.data_case_split_risk
    }

    /// #597: the axioms conversion dropped (kind → count); empty when none. A
    /// justification is computed over the weaker ontology, so one may be missing.
    /// `PreparedOntology.justify` / `justify_all` emit `DroppedAxiomsWarning`.
    #[getter]
    fn dropped(&self) -> std::collections::BTreeMap<String, u64> {
        self.inner.dropped().by_kind().clone()
    }

    /// One justification for a query (CLI-style tokens; see `justify`) as the module
    /// `justify`'s dict (#625). The Python package binds `PreparedOntology.justify` /
    /// `justify_result` wrappers over this that warn.
    #[pyo3(name = "_justify_reporting")]
    #[allow(clippy::needless_pass_by_value)]
    fn justify_reporting<'py>(
        &self,
        py: Python<'py>,
        query: Vec<String>,
    ) -> PyResult<Bound<'py, PyDict>> {
        let q = owl_dl_reasoner::justify::parse_query(&query).map_err(PyValueError::new_err)?;
        let outcome = self
            .inner
            .find_one_reporting(&q)
            .map_err(reason_error_to_py)?;
        outcome_dict(py, &outcome, None)
    }

    /// Up to `max` justifications as the module `justify_all`'s dict (#625); wrapped by
    /// `PreparedOntology.justify_all` / `justify_all_result`.
    #[pyo3(name = "_justify_all_reporting", signature = (query, max = 10))]
    #[allow(clippy::needless_pass_by_value)]
    fn justify_all_reporting<'py>(
        &self,
        py: Python<'py>,
        query: Vec<String>,
        max: usize,
    ) -> PyResult<Bound<'py, PyDict>> {
        let q = owl_dl_reasoner::justify::parse_query(&query).map_err(PyValueError::new_err)?;
        let outcome = self
            .inner
            .find_all_reporting(&q, max)
            .map_err(reason_error_to_py)?;
        outcome_dict(py, &outcome, Some(max))
    }
}

/// `rustdl.prepare(path)` — load an ontology and precompute its justification
/// state, returning a reusable `PreparedOntology`.
#[pyfunction]
pub(crate) fn prepare(path: &str) -> PyResult<PyPreparedOntology> {
    let onto = load::load_path(path)?;
    let (universal_role_ordinary, data_case_split_risk) = prepared_flags(&onto);
    Ok(PyPreparedOntology {
        inner: owl_dl_reasoner::justify::PreparedJustifier::prepare(&onto),
        universal_role_ordinary,
        data_case_split_risk,
    })
}

/// `rustdl.prepare_bytes(data, *, format)` — same, from in-memory bytes.
/// `format` is required and keyword-only (as on `classify_bytes`): one of
/// `"ofn"`, `"owx"`, `"rdf-xml"`, `"omn"`.
#[pyfunction]
#[pyo3(signature = (data, *, format))]
pub(crate) fn prepare_bytes(data: &[u8], format: &str) -> PyResult<PyPreparedOntology> {
    let onto = load::load_bytes(data, format)?;
    let (universal_role_ordinary, data_case_split_risk) = prepared_flags(&onto);
    Ok(PyPreparedOntology {
        inner: owl_dl_reasoner::justify::PreparedJustifier::prepare(&onto),
        universal_role_ordinary,
        data_case_split_risk,
    })
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(justify, m)?)?;
    m.add_function(wrap_pyfunction!(justify_all, m)?)?;
    m.add_function(wrap_pyfunction!(diagnose, m)?)?;
    m.add_function(wrap_pyfunction!(repair, m)?)?;
    m.add_function(wrap_pyfunction!(render_manchester, m)?)?;
    m.add_class::<PyPreparedOntology>()?;
    m.add_function(wrap_pyfunction!(prepare, m)?)?;
    m.add_function(wrap_pyfunction!(prepare_bytes, m)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A⊑B, B⊑C written to a temp `.ofn` fixture; `render_manchester` should
    /// return one Manchester string per `SubClassOf` axiom, each mentioning
    /// its sub/super class, and skip the two `Declaration(Class(...))`
    /// components as non-logical noise.
    #[test]
    fn render_manchester_renders_subclass_chain() {
        let src = "Prefix(:=<http://x/>)\n\
Ontology(\n\
Declaration(Class(:A))\n\
Declaration(Class(:B))\n\
Declaration(Class(:C))\n\
SubClassOf(:A :B)\n\
SubClassOf(:B :C)\n\
)\n";
        let path = std::env::temp_dir().join("rustdl_render_manchester_test.ofn");
        std::fs::write(&path, src).expect("write fixture");
        let path_str = path.to_str().expect("utf8 path").to_string();

        let result = render_manchester(&path_str).expect("render_manchester");

        std::fs::remove_file(&path).ok();

        assert_eq!(
            result.len(),
            2,
            "expected exactly the two SubClassOf axioms, got {result:?}"
        );
        assert!(
            result.iter().any(|s| s.contains('A') && s.contains('B')),
            "missing A⊑B in {result:?}"
        );
        assert!(
            result.iter().any(|s| s.contains('B') && s.contains('C')),
            "missing B⊑C in {result:?}"
        );
    }
}
