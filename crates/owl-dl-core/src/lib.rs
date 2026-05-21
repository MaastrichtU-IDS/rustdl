//! Core IR, normalization, and shared utilities for the rustdl OWL DL reasoner.
//!
//! See `docs/architecture.md` and the project-level strategy document for the
//! big-picture design. This crate hosts:
//!
//! - The interned concept / role / individual IR (`ir` module — coming in Phase 0).
//! - Normalization passes (NNF, structural transformation — Phase 1).
//! - Absorption (Phase 1).
//! - Told-subsumer and told-disjoint tables (Phase 1).
//!
//! Nothing here yet — Day 1-2 is workspace scaffolding only.
