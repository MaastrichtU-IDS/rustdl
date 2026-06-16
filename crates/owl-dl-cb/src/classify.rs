//! Read the atomic-class hierarchy off the saturated context graph (Task B).

use crate::CbHierarchy;
use crate::model::ContextGraph;
use crate::normalize::Normalized;

/// Read the (transitively closed) atomic-class subsumption relation + the
/// unsatisfiable set from the saturated graph.
#[must_use]
pub fn read_hierarchy(_norm: &Normalized, _graph: &ContextGraph) -> CbHierarchy {
    todo!("Task B: hierarchy read-off")
}
