//! Laconic (fine-grained) justifications: weaken each axiom of a regular
//! justification to its responsible fragment, then re-minimize. Sound by
//! construction — every emitted fragment is *entailed by* an original axiom, so a
//! laconic justification is a set of genuine consequences of the ontology that
//! explains the entailment. Read-only; FP=0 untouched.

use std::collections::{BTreeSet, HashSet};

use horned_owl::model::{ClassExpression, Component, ForIRI, SubClassOf};
use horned_owl::ontology::set::SetOntology;

use crate::ReasonError;
use crate::justify::{
    Entailment, Justification, find_all_justifications, find_one_justification, logical_axioms,
    quickxplain,
};

/// Weaken a single axiom into a set of fragments, each ENTAILED BY the axiom.
/// An axiom with no applicable operator returns `vec![axiom.clone()]` (passes
/// through unchanged). Filled in by Task 2.
#[allow(dead_code)] // wired into the driver in Task 3; allow removed there
fn weaken<A: ForIRI>(axiom: &Component<A>) -> Vec<Component<A>> {
    vec![axiom.clone()]
}

/// Laconic justification for `q` (one). Filled in by Task 3.
pub fn find_laconic_justification<A: ForIRI>(
    onto: &SetOntology<A>,
    q: &Entailment,
) -> Result<Option<Justification<A>>, ReasonError> {
    let _ = (onto, q);
    Ok(None)
}

/// All laconic justifications for `q` (capped). Filled in by Task 3.
pub fn find_all_laconic_justifications<A: ForIRI>(
    onto: &SetOntology<A>,
    q: &Entailment,
    max: usize,
) -> Result<Vec<Justification<A>>, ReasonError> {
    let _ = (onto, q, max);
    Ok(Vec::new())
}
