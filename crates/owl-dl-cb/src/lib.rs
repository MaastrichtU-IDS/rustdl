//! Consequence-based ALCH classification engine (Architecture B, slice B1).
//!
//! A global, saturation-style classifier — no per-pair satisfiability probing,
//! no tableau, no backtracking. Sound AND complete for ALCH; returns
//! [`CbOutcome::OutOfFragment`] for any input using a construct outside ALCH
//! (`≤n`/`≥n`, inverse roles, nominals, datatypes, role chains/transitivity,
//! `Self`). Run side by side with the per-pair hybrid for comparison.
//!
//! Spec: `docs/superpowers/specs/2026-06-15-cb-engine-b1-alch-design.md`.

mod classify;
mod engine;
mod model;
mod normalize;
mod seq_classify;
mod seq_engine;
mod seq_model;
mod seq_order;

pub use model::{
    Atom, Context, ContextGraph, ContextId, DerivedClause, EdgeKind, HeadLit, Literal, OntClause,
    Term, TermId,
};

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

/// Which CB calculus to run: the default unordered B1/B2 engine, or the
/// Sequoia ordered calculus (S1, ALCH only). Selected via the
/// `RUSTDL_CB_CALCULUS` env var (`unordered` [default] | `sequoia`).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Calculus {
    Unordered,
    Sequoia,
}

fn selected_calculus() -> Calculus {
    match std::env::var("RUSTDL_CB_CALCULUS").as_deref() {
        Ok("sequoia") => Calculus::Sequoia,
        _ => Calculus::Unordered,
    }
}

/// Does any normalized clause carry a `≤n`/`≥n` literal (a body or head atom
/// whose interned concept is `Min`/`Max`)? S1 of the ordered calculus covers
/// ALCH ONLY, so such a clause routes to `OutOfFragment` on the Sequoia path.
fn has_cardinality(norm: &normalize::Normalized) -> bool {
    use owl_dl_core::ir::ConceptExpr;
    norm.clauses.iter().any(|cl| {
        cl.premise.iter().chain(cl.head.iter()).any(|&c| {
            matches!(
                norm.pool.get(c),
                ConceptExpr::Min(..) | ConceptExpr::Max(..)
            )
        })
    })
}

/// Classify `internal` with the consequence-based engine. Dispatches on
/// `RUSTDL_CB_CALCULUS` (`unordered` [default] | `sequoia`).
#[must_use]
pub fn classify(internal: &InternalOntology) -> CbOutcome {
    match selected_calculus() {
        Calculus::Sequoia => classify_sequoia(internal),
        Calculus::Unordered => classify_unordered(internal),
    }
}

/// Classify with the unordered B1/B2 engine (the default, sound+complete on
/// ALCH/ALCHQ). The differential oracle for the Sequoia engine.
#[must_use]
pub fn classify_unordered(internal: &InternalOntology) -> CbOutcome {
    let dbg = std::env::var("RUSTDL_CB_DEBUG").is_ok();
    match normalize::normalize(internal) {
        Err(reason) => CbOutcome::OutOfFragment(reason),
        Ok(norm) => {
            if dbg {
                eprintln!(
                    "[cb] normalize done: {} clauses, {} classes; calculus=unordered",
                    norm.clauses.len(),
                    norm.classes.len()
                );
            }
            let graph = engine::saturate(&norm);
            CbOutcome::Classified(classify::read_hierarchy(&norm, &graph))
        }
    }
}

/// Classify with the Sequoia ordered calculus (S1, ALCH only). Routes ALCHQ
/// (`≤n`/`≥n`) — which the shared `normalize` gate admits — to `OutOfFragment`
/// since S1 covers ALCH only (cardinality lands in S2).
#[must_use]
pub fn classify_sequoia(internal: &InternalOntology) -> CbOutcome {
    let dbg = std::env::var("RUSTDL_CB_DEBUG").is_ok();
    match normalize::normalize(internal) {
        Err(reason) => CbOutcome::OutOfFragment(reason),
        Ok(norm) => {
            if dbg {
                eprintln!(
                    "[seq] normalize done: {} clauses, {} classes; calculus=sequoia",
                    norm.clauses.len(),
                    norm.classes.len()
                );
            }
            if has_cardinality(&norm) {
                return CbOutcome::OutOfFragment("cardinality (S2+)");
            }
            let graph = seq_engine::saturate(&norm);
            CbOutcome::Classified(seq_classify::read_hierarchy(&norm, &graph))
        }
    }
}
