//! Core IR, normalization, and shared utilities for the rustdl OWL DL reasoner.
//!
//! Phase 0 (in progress) lands the interned concept IR with structural sharing.
//! Phase 1 will add NNF, structural transformation, absorption, told-subsumers,
//! and told-disjoints.
//!
//! See `owl-dl-reasoner-rust-strategy-v2.md` at the workspace root for the
//! full plan.

pub mod convert;
pub mod convert_back;
pub mod ir;
pub mod ontology;
pub mod role_hierarchy;
pub mod vocab;

pub use convert::{
    ConversionError, convert_class_expression, convert_component, convert_individual,
    convert_object_property, convert_ontology,
};
pub use convert_back::{axiom_to_component, concept_to_class_expression, convert_back};
pub use ir::{ClassId, ConceptExpr, ConceptId, ConceptPool, IndividualId, Role, RoleId};
pub use ontology::{Axiom, InternalOntology, SubRolePath};
pub use role_hierarchy::{RoleHierarchy, RoleHierarchyBuilder};
pub use vocab::Vocabulary;
