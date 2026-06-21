//! Root/derived unsatisfiability diagnosis: partition the classified
//! unsatisfiable classes into *root* causes and *derived* collateral, using a
//! stingy structural dependency graph (edges only for unsat-forcing positions).
//! Read-only over classification — adds no entailments, so FP=0 is untouched.

use std::collections::{BTreeMap, BTreeSet};

use horned_owl::model::{ClassExpression, Component, ForIRI};
use horned_owl::ontology::set::SetOntology;

use crate::ReasonError;

/// A derived unsatisfiable class and the root cause(s) it depends on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DerivedClass {
    /// IRI of the derived (collateral) unsatisfiable class.
    pub iri: String,
    /// IRIs of the root class(es) it transitively depends on.
    pub roots: Vec<String>,
}

/// The result of diagnosing an ontology.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnosis {
    /// Whether the ontology is consistent. When `false`, `roots`/`derived` are
    /// empty and the caller should justify the inconsistency directly.
    pub consistent: bool,
    /// Root unsatisfiable classes (IRIs), sorted. Empty if consistent and coherent.
    pub roots: Vec<String>,
    /// Derived unsatisfiable classes, each with its root(s), sorted by IRI.
    pub derived: Vec<DerivedClass>,
    /// Every unsatisfiable class (roots ∪ derived), sorted — the conservation set.
    pub all_unsat: Vec<String>,
    /// For each root IRI, the derived classes that depend on it, sorted.
    pub root_derives: BTreeMap<String, Vec<String>>,
}

/// Diagnose `onto`: consistency, then the root/derived unsatisfiability partition.
///
/// Read-only over classification; never mutates the ontology.
pub fn diagnose<A: ForIRI>(onto: &SetOntology<A>) -> Result<Diagnosis, ReasonError> {
    // Filled in by Task 4.
    let _ = onto;
    Ok(Diagnosis {
        consistent: true,
        roots: Vec::new(),
        derived: Vec::new(),
        all_unsat: Vec::new(),
        root_derives: BTreeMap::new(),
    })
}
