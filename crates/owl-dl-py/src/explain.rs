//! Python bindings for the explanation/debugging suite (justify, diagnose, repair).
//! String/tuple forms; axioms rendered as Manchester with full IRIs.

use horned_owl::curie::PrefixMapping;
use horned_owl::io::omn::AsManchester;
use horned_owl::model::{Component, RcStr};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

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

/// The first justification's axioms as Manchester strings (empty if none),
/// with the outcome's give-up flag (#396).
fn first_rendered(
    outcome: &owl_dl_reasoner::justify::JustifyOutcome<RcStr>,
) -> (Vec<String>, bool) {
    let axioms = outcome
        .justifications
        .first()
        .map(|j| j.axioms.iter().map(render).collect())
        .unwrap_or_default();
    (axioms, outcome.incomplete)
}

/// Every justification's axioms as Manchester strings, with the outcome's
/// give-up flag (#396).
fn all_rendered(
    outcome: &owl_dl_reasoner::justify::JustifyOutcome<RcStr>,
) -> (Vec<Vec<String>>, bool) {
    let js = outcome
        .justifications
        .iter()
        .map(|j| j.axioms.iter().map(render).collect())
        .collect();
    (js, outcome.incomplete)
}

/// One minimal justification for a query (CLI-style tokens, e.g.
/// `["subclass", sub, sup]`, `["unsat", c]`, `["inconsistent"]`) as Manchester
/// axiom strings — empty if not entailed — paired with `incomplete`: a reasoner
/// check gave up (#396), so an empty list means "unknown", and a non-empty one
/// may not be minimal. The `rustdl.justify` wrapper turns the flag into an
/// `IncompleteQueryWarning`.
#[pyfunction]
#[allow(clippy::needless_pass_by_value)]
pub(crate) fn justify(path: &str, query: Vec<String>) -> PyResult<(Vec<String>, bool)> {
    let onto = load::load_path(path)?;
    let q = owl_dl_reasoner::justify::parse_query(&query).map_err(PyValueError::new_err)?;
    let outcome = owl_dl_reasoner::justify::find_one_justification_reporting(&onto, &q)
        .map_err(reason_error_to_py)?;
    Ok(first_rendered(&outcome))
}

/// All minimal justifications (capped by `max`), paired with `incomplete` (see
/// `justify`; here it also means some justifications may be missing).
#[pyfunction]
#[pyo3(signature = (path, query, max = 10))]
#[allow(clippy::needless_pass_by_value)]
pub(crate) fn justify_all(
    path: &str,
    query: Vec<String>,
    max: usize,
) -> PyResult<(Vec<Vec<String>>, bool)> {
    let onto = load::load_path(path)?;
    let q = owl_dl_reasoner::justify::parse_query(&query).map_err(PyValueError::new_err)?;
    let outcome = owl_dl_reasoner::justify::find_all_justifications_reporting(&onto, &q, max)
        .map_err(reason_error_to_py)?;
    Ok(all_rendered(&outcome))
}

/// Root/derived unsatisfiability partition:
/// `(consistent, roots, [(derived_iri, [root_iri, ...]), ...], incomplete)`.
/// `incomplete` (#396): the consistency check of an all-classes-unsatisfiable
/// ontology gave up (`consistent` is not witnessed), or a class's
/// unsatisfiability check hit the node cap (unsatisfiable classes may be
/// missing).
#[pyfunction]
#[allow(clippy::type_complexity)]
pub(crate) fn diagnose(
    path: &str,
) -> PyResult<(bool, Vec<String>, Vec<(String, Vec<String>)>, bool)> {
    let onto = load::load_path(path)?;
    let d = owl_dl_reasoner::diagnose(&onto).map_err(reason_error_to_py)?;
    let derived = d.derived.into_iter().map(|dc| (dc.iri, dc.roots)).collect();
    Ok((d.consistent, d.roots, derived, d.incomplete))
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
) -> PyResult<(Vec<Vec<String>>, bool)> {
    let onto = load::load_path(path)?;
    let q = owl_dl_reasoner::justify::parse_query(&query).map_err(PyValueError::new_err)?;
    let r = owl_dl_reasoner::find_repairs(&onto, &q, max).map_err(reason_error_to_py)?;
    let repairs = r
        .repairs
        .into_iter()
        .map(|rep| rep.remove.iter().map(render).collect())
        .collect();
    Ok((repairs, r.incomplete))
}

/// A loaded ontology with the per-ontology justification state precomputed once
/// (parse + logical-axiom split + fragment classification). Reuse it to justify
/// many queries without re-doing that full-ontology work each call — a large
/// win on big ontologies. Unsendable: use it from the thread that created it.
#[pyclass(name = "PreparedOntology", module = "rustdl", unsendable)]
pub(crate) struct PyPreparedOntology {
    inner: owl_dl_reasoner::justify::PreparedJustifier<RcStr>,
}

#[pymethods]
impl PyPreparedOntology {
    /// One minimal justification for a query (CLI-style tokens; see `justify`),
    /// paired with `incomplete` (#396). The Python package binds a
    /// `PreparedOntology.justify` wrapper over this that warns instead.
    #[pyo3(name = "_justify_reporting")]
    #[allow(clippy::needless_pass_by_value)]
    fn justify_reporting(&self, query: Vec<String>) -> PyResult<(Vec<String>, bool)> {
        let q = owl_dl_reasoner::justify::parse_query(&query).map_err(PyValueError::new_err)?;
        let outcome = self
            .inner
            .find_one_reporting(&q)
            .map_err(reason_error_to_py)?;
        Ok(first_rendered(&outcome))
    }

    /// All minimal justifications (capped by `max`), paired with `incomplete`;
    /// wrapped by `PreparedOntology.justify_all`.
    #[pyo3(name = "_justify_all_reporting", signature = (query, max = 10))]
    #[allow(clippy::needless_pass_by_value)]
    fn justify_all_reporting(
        &self,
        query: Vec<String>,
        max: usize,
    ) -> PyResult<(Vec<Vec<String>>, bool)> {
        let q = owl_dl_reasoner::justify::parse_query(&query).map_err(PyValueError::new_err)?;
        let outcome = self
            .inner
            .find_all_reporting(&q, max)
            .map_err(reason_error_to_py)?;
        Ok(all_rendered(&outcome))
    }
}

/// `rustdl.prepare(path)` — load an ontology and precompute its justification
/// state, returning a reusable `PreparedOntology`.
#[pyfunction]
pub(crate) fn prepare(path: &str) -> PyResult<PyPreparedOntology> {
    let onto = load::load_path(path)?;
    Ok(PyPreparedOntology {
        inner: owl_dl_reasoner::justify::PreparedJustifier::prepare(&onto),
    })
}

/// `rustdl.prepare_bytes(data, *, format)` — same, from in-memory bytes.
/// `format` is required and keyword-only (as on `classify_bytes`): one of
/// `"ofn"`, `"owx"`, `"rdf-xml"`, `"omn"`.
#[pyfunction]
#[pyo3(signature = (data, *, format))]
pub(crate) fn prepare_bytes(data: &[u8], format: &str) -> PyResult<PyPreparedOntology> {
    let onto = load::load_bytes(data, format)?;
    Ok(PyPreparedOntology {
        inner: owl_dl_reasoner::justify::PreparedJustifier::prepare(&onto),
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
