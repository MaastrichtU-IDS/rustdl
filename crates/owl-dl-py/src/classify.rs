//! `classify` / `classify_bytes` top-level functions + the
//! `Classification` PyO3 class that wraps `owl_dl_reasoner::Classification`.

use owl_dl_reasoner::{Classification as RsClassification, classify as rs_classify};
use pyo3::prelude::*;

use crate::errors::reason_error_to_py;
use crate::load;

/// Wraps the Rust-side `Classification` for Python consumption.
#[pyclass(name = "Classification", module = "rustdl")]
pub(crate) struct PyClassification {
    inner: RsClassification,
}

#[pymethods]
impl PyClassification {
    /// All declared class IRIs in the ontology, insertion order.
    #[getter]
    fn classes(&self) -> Vec<String> {
        self.inner.classes().to_vec()
    }

    /// IRIs proved unsatisfiable (⊑ ⊥). Sorted ascending.
    #[getter]
    fn unsatisfiable(&self) -> Vec<String> {
        self.inner
            .unsatisfiable_classes()
            .into_iter()
            .map(String::from)
            .collect()
    }

    /// True iff the whole ontology was flagged inconsistent.
    /// (Set by the Phase-A1 ABox consistency check.)
    #[getter]
    fn inconsistent(&self) -> bool {
        self.inner.stats().inconsistent
    }

    /// Number of class pairs that exceeded the per-pair timeout and
    /// were recorded as "not subsumed". `> 0` means the classification
    /// is a sound under-approximation — no false subsumptions, but real
    /// ones may be missing. `0` when classification ran to completion
    /// (or no timeout was set).
    #[getter]
    fn timed_out_pairs(&self) -> usize {
        self.inner.stats().timed_out_pairs
    }

    /// True iff classification ran to completion — no pair hit the
    /// timeout. When `False`, the hierarchy may be missing real
    /// subsumptions (see `timed_out_pairs`); re-classify with
    /// `per_pair_timeout_ms=0` for the complete (unbounded) result.
    #[getter]
    fn complete(&self) -> bool {
        self.inner.stats().timed_out_pairs == 0
    }

    /// True iff `sub ⊑ sup` is entailed.
    fn is_subclass(&self, sub: &str, sup: &str) -> bool {
        self.inner.is_subclass(sub, sup)
    }

    /// All classes equivalent to `cls` (including `cls` itself).
    fn equivalent_classes(&self, cls: &str) -> Vec<String> {
        self.inner
            .equivalent_classes(cls)
            .into_iter()
            .map(String::from)
            .collect()
    }

    /// The Hasse-direct super-classes of `cls`. (Direct, not transitive.)
    fn direct_subsumers(&self, cls: &str) -> Vec<String> {
        self.inner
            .direct_subsumers(cls)
            .into_iter()
            .map(String::from)
            .collect()
    }

    fn __repr__(&self) -> String {
        format!(
            "Classification(classes={}, unsatisfiable={}, inconsistent={})",
            self.inner.classes().len(),
            self.inner.unsatisfiable_classes().len(),
            self.inner.stats().inconsistent,
        )
    }
}

/// `rustdl.classify(path)` — classify the ontology at `path`.
/// Format auto-detected from the file extension.
#[pyfunction]
#[pyo3(signature = (path, *, per_pair_timeout_ms=1000, saturation_only=false))]
pub(crate) fn classify(
    path: &str,
    per_pair_timeout_ms: Option<u64>,
    saturation_only: bool,
) -> PyResult<PyClassification> {
    let ontology = load::load_path(path)?;
    do_classify(&ontology, per_pair_timeout_ms, saturation_only)
}

/// `rustdl.classify_bytes(data, format="ofn")` — same but from bytes.
#[pyfunction]
#[pyo3(signature = (data, *, format, per_pair_timeout_ms=1000, saturation_only=false))]
pub(crate) fn classify_bytes(
    data: &[u8],
    format: &str,
    per_pair_timeout_ms: Option<u64>,
    saturation_only: bool,
) -> PyResult<PyClassification> {
    let ontology = load::load_bytes(data, format)?;
    do_classify(&ontology, per_pair_timeout_ms, saturation_only)
}

fn do_classify(
    ontology: &horned_owl::ontology::set::SetOntology<horned_owl::model::RcStr>,
    per_pair_timeout_ms: Option<u64>,
    saturation_only: bool,
) -> PyResult<PyClassification> {
    use std::time::Duration;
    // `None` or `0` → unbounded (complete); any positive value bounds
    // each pair (sound under-approximation; check `.complete` after).
    let bounded = per_pair_timeout_ms.filter(|&ms| ms > 0);
    let inner = if saturation_only {
        owl_dl_reasoner::classify_saturation_only(ontology).map_err(reason_error_to_py)?
    } else if let Some(ms) = bounded {
        owl_dl_reasoner::classify_top_down_with_timeout(ontology, Duration::from_millis(ms))
            .map_err(reason_error_to_py)?
    } else {
        rs_classify(ontology).map_err(reason_error_to_py)?
    };
    Ok(PyClassification { inner })
}

/// Register module bindings.
pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyClassification>()?;
    m.add_function(wrap_pyfunction!(classify, m)?)?;
    m.add_function(wrap_pyfunction!(classify_bytes, m)?)?;
    Ok(())
}
