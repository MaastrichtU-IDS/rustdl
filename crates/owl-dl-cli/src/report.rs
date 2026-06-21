//! `rustdl report`: a self-contained HTML debugging report (summary + diagnose
//! roots/derived + per-root justification + repairs). Presentation-only over the
//! shipped reasoner output; runs no new reasoning; read-only.

use horned_owl::model::{Component, RcStr};

/// One root unsatisfiable class with its explanation and fixes.
pub struct RootEntry {
    pub iri: String,
    pub justification: Vec<Component<RcStr>>,
    pub repairs: Vec<Vec<Component<RcStr>>>,
    pub derives: Vec<String>,
}

/// The inconsistency explanation (when the whole ontology is inconsistent).
pub struct Section {
    pub justification: Vec<Component<RcStr>>,
    pub repairs: Vec<Vec<Component<RcStr>>>,
}

/// Everything the HTML renderer needs — assembled by `build_report`.
pub struct Report {
    pub ontology_path: String,
    pub class_count: usize,
    pub consistent: bool,
    pub fragment: String,
    pub inconsistency: Option<Section>,
    pub roots: Vec<RootEntry>,
    pub derived: Vec<(String, Vec<String>)>,
    pub n_unsat: usize,
    pub n_root: usize,
    pub n_derived: usize,
    pub repairs_complete: bool,
    pub truncated_roots: usize,
}
