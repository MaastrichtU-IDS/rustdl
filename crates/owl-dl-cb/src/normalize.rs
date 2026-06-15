//! IR → ALCH clausal normal form + the ALCH fragment gate (Task A).

use crate::model::OntClause;
use owl_dl_core::ir::{ClassId, ConceptPool, Role};
use owl_dl_core::ontology::InternalOntology;

/// A normalized ALCH ontology: clausal axioms + the reportable atomic-class
/// vocabulary + the role hierarchy (for `∀`-propagation) + the (possibly
/// extended) concept pool.
pub struct Normalized {
    pub clauses: Vec<OntClause>,
    /// Reportable atomic classes (excludes definitional/synthetic atoms).
    pub classes: Vec<ClassId>,
    /// `R ⊑ S` edges (used by the engine's `∀`-propagation).
    pub role_hierarchy: Vec<(Role, Role)>,
    /// Owned pool; may gain definitional atoms from the structural transform.
    pub pool: ConceptPool,
}

/// Normalize `internal` to ALCH clausal form, or `Err(reason)` naming the first
/// out-of-ALCH construct encountered.
pub fn normalize(_internal: &InternalOntology) -> Result<Normalized, &'static str> {
    todo!("Task A: clausal normalization + ALCH fragment gate")
}
