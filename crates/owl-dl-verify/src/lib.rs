//! Verified canonical models for pure-EL ontologies, and an engine-blind
//! axiom evaluator over them.
//!
//! See `docs/superpowers/specs/2026-08-27-negative-certificates-phase1-design.md`.

pub mod eval;
pub mod interp;
pub mod model;

pub use interp::{Element, Interpretation};

use owl_dl_core::{ClassId, RoleId};

/// Construction bounds. Checking is bounded separately, by a deadline passed to
/// `verify`, so no stale `Instant` is ever read off a model.
#[derive(Clone, Debug)]
pub struct Bounds {
    pub max_elements: usize,
    pub max_edges: usize,
    pub max_rounds: usize,
}

impl Default for Bounds {
    fn default() -> Self {
        Self {
            max_elements: 50_000,
            max_edges: 2_000_000,
            max_rounds: 8,
        }
    }
}

/// Why a run could not reach a verdict. NEVER treated as `Verified`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UnresolvedReason {
    UnhandledAxiom {
        axiom_index: usize,
        variant: &'static str,
    },
    UnhandledConcept {
        axiom_index: usize,
        variant: &'static str,
    },
    /// `limit: None` means a deadline expired rather than a count being exceeded.
    BoundTripped {
        bound: &'static str,
        limit: Option<usize>,
    },
    GuardedRoleHasEdges {
        role: RoleId,
    },
    ChainRangeOutOfProfile {
        chain_super: RoleId,
    },
    LabelNotClosed {
        class: ClassId,
        role: RoleId,
    },
    /// A run-delta on an ORIGINAL class between the first and final saturation:
    /// direct evidence the shipped classification is incomplete.
    RunDelta {
        class: ClassId,
    },
}
