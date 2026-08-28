//! Verified canonical models for pure-EL ontologies, and an engine-blind
//! axiom evaluator over them.
//!
//! See `docs/superpowers/specs/2026-08-27-negative-certificates-phase1-design.md`.

pub mod eval;
pub mod interp;
pub mod model;

pub use interp::{Element, Interpretation};
