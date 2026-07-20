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

/// One minimal justification for a query (CLI-style tokens, e.g.
/// `["subclass", sub, sup]`, `["unsat", c]`, `["inconsistent"]`) as Manchester
/// axiom strings. Empty list if not entailed.
#[pyfunction]
#[allow(clippy::needless_pass_by_value)]
pub(crate) fn justify(path: &str, query: Vec<String>) -> PyResult<Vec<String>> {
    let onto = load::load_path(path)?;
    let q = owl_dl_reasoner::justify::parse_query(&query).map_err(PyValueError::new_err)?;
    let j =
        owl_dl_reasoner::justify::find_one_justification(&onto, &q).map_err(reason_error_to_py)?;
    Ok(j.map(|j| j.axioms.iter().map(render).collect())
        .unwrap_or_default())
}

/// All minimal justifications (capped by `max`).
#[pyfunction]
#[pyo3(signature = (path, query, max = 10))]
#[allow(clippy::needless_pass_by_value)]
pub(crate) fn justify_all(
    path: &str,
    query: Vec<String>,
    max: usize,
) -> PyResult<Vec<Vec<String>>> {
    let onto = load::load_path(path)?;
    let q = owl_dl_reasoner::justify::parse_query(&query).map_err(PyValueError::new_err)?;
    let js = owl_dl_reasoner::justify::find_all_justifications(&onto, &q, max)
        .map_err(reason_error_to_py)?;
    Ok(js
        .into_iter()
        .map(|j| j.axioms.iter().map(render).collect())
        .collect())
}

/// Root/derived unsatisfiability partition:
/// `(consistent, roots, [(derived_iri, [root_iri, ...]), ...])`.
#[pyfunction]
#[allow(clippy::type_complexity)]
pub(crate) fn diagnose(path: &str) -> PyResult<(bool, Vec<String>, Vec<(String, Vec<String>)>)> {
    let onto = load::load_path(path)?;
    let d = owl_dl_reasoner::diagnose(&onto).map_err(reason_error_to_py)?;
    let derived = d.derived.into_iter().map(|dc| (dc.iri, dc.roots)).collect();
    Ok((d.consistent, d.roots, derived))
}

/// Minimal repairs for a query: each is a list of Manchester axioms to remove.
#[pyfunction]
#[pyo3(signature = (path, query, max = 10))]
#[allow(clippy::needless_pass_by_value)]
pub(crate) fn repair(path: &str, query: Vec<String>, max: usize) -> PyResult<Vec<Vec<String>>> {
    let onto = load::load_path(path)?;
    let q = owl_dl_reasoner::justify::parse_query(&query).map_err(PyValueError::new_err)?;
    let r = owl_dl_reasoner::find_repairs(&onto, &q, max).map_err(reason_error_to_py)?;
    Ok(r.repairs
        .into_iter()
        .map(|rep| rep.remove.iter().map(render).collect())
        .collect())
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(justify, m)?)?;
    m.add_function(wrap_pyfunction!(justify_all, m)?)?;
    m.add_function(wrap_pyfunction!(diagnose, m)?)?;
    m.add_function(wrap_pyfunction!(repair, m)?)?;
    m.add_function(wrap_pyfunction!(render_manchester, m)?)?;
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
