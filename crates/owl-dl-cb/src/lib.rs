//! Consequence-based ALCH classification engine (Architecture B, slice B1).
//!
//! A global, saturation-style classifier — no per-pair satisfiability probing,
//! no tableau, no backtracking. Sound AND complete for ALCH; returns
//! [`CbOutcome::OutOfFragment`] for any input using a construct outside ALCH
//! (`≤n`/`≥n`, inverse roles, nominals, datatypes, role chains/transitivity,
//! `Self`). Run side by side with the per-pair hybrid for comparison.
//!
//! Spec: `docs/superpowers/specs/2026-06-15-cb-engine-b1-alch-design.md`.

// Staged-build allowance: the frozen interface (Task 0) carries fields/items
// that are not read until the engine + normalizer land (Tasks A/B). Removed at
// integration (Task E) once everything is wired and exercised.
#![allow(dead_code, unreachable_pub)]

mod classify;
mod engine;
mod model;
mod normalize;

pub use model::{Atom, Context, ContextGraph, ContextId, DerivedClause, Literal, OntClause};

use owl_dl_core::ir::ClassId;
use owl_dl_core::ontology::InternalOntology;
use std::collections::BTreeSet;

/// Outcome of a consequence-based classification attempt.
pub enum CbOutcome {
    /// ALCH input: a sound + complete atomic-class hierarchy.
    Classified(CbHierarchy),
    /// Input uses a construct outside ALCH — the caller must defer to another
    /// engine. The `&'static str` names the offending construct.
    OutOfFragment(&'static str),
}

/// Atomic-class subsumption result, comparable to the reasoner's
/// `Classification`.
#[derive(Debug, Default)]
pub struct CbHierarchy {
    /// `(sub, sup)` atomic-class pairs with `sub ⊑ sup`, transitively closed,
    /// excluding reflexive pairs and `owl:Thing`/`owl:Nothing` on either side.
    pub subsumptions: BTreeSet<(ClassId, ClassId)>,
    /// Classes proven unsatisfiable.
    pub unsat: BTreeSet<ClassId>,
}

/// Classify `internal` with the consequence-based engine.
#[must_use]
pub fn classify(internal: &InternalOntology) -> CbOutcome {
    match normalize::normalize(internal) {
        Err(reason) => CbOutcome::OutOfFragment(reason),
        Ok(norm) => {
            let graph = engine::saturate(&norm);
            CbOutcome::Classified(classify::read_hierarchy(&norm, &graph))
        }
    }
}
