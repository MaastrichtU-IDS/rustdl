//! The consequence-based ALCH calculus: context saturation (Task B).

use crate::model::ContextGraph;
use crate::normalize::Normalized;

/// Saturate the context graph under the consequence-based ALCH inference rules
/// (core resolution, ordered `⊔` resolution, `∃`-Succ, `∀`-Pred, `⊥`) to a
/// fixpoint.
#[must_use]
pub fn saturate(_norm: &Normalized) -> ContextGraph {
    todo!("Task B: consequence-based ALCH saturation")
}
