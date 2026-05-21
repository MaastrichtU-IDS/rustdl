//! Core IR, normalization, and shared utilities for the rustdl OWL DL reasoner.
//!
//! Phase 0 (in progress) lands the interned concept IR with structural sharing.
//! Phase 1 will add NNF, structural transformation, absorption, told-subsumers,
//! and told-disjoints.
//!
//! See `owl-dl-reasoner-rust-strategy-v2.md` at the workspace root for the
//! full plan.

pub mod ir;
pub mod role_hierarchy;
pub mod vocab;

pub use ir::{ClassId, ConceptExpr, ConceptId, ConceptPool, IndividualId, Role, RoleId};
pub use role_hierarchy::{RoleHierarchy, RoleHierarchyBuilder};
pub use vocab::Vocabulary;
