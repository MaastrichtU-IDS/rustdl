//! Core IR, normalization, and shared utilities for the rustdl OWL DL reasoner.
//!
//! Phase 0 (in progress) lands the interned concept IR with structural sharing.
//! Phase 1 will add NNF, structural transformation, absorption, told-subsumers,
//! and told-disjoints.
//!
//! See `owl-dl-reasoner-rust-strategy-v2.md` at the workspace root for the
//! full plan.

pub mod absorb;
pub mod approx_saturation;
pub mod clause;
pub mod convert;
pub mod convert_back;
pub mod data_axioms;
pub mod definitions;
pub mod delta;
pub mod disjunction_existential;
pub mod disjunctive_antecedent;
pub mod ir;
pub mod locality;
pub mod normalize;
pub mod ontology;
pub mod render;
pub mod residual_trigger;
pub mod role_hierarchy;
pub mod signature;
pub mod told;
pub mod transform;
pub mod vocab;

pub use absorb::{AbsorbedTBox, ConceptRule, NominalRule, RoleRule, absorb, absorb_roles};
pub use convert::{
    ConversionError, DKEY_IRI_PREFIX, convert_class_expression, convert_component,
    convert_individual, convert_object_property, convert_ontology, convert_ontology_seeded,
    decode_date_dkey, decode_date_oneof_dkey, decode_datetime_dkey, decode_datetime_oneof_dkey,
    decode_decimal_dkey, decode_decimal_oneof_dkey, decode_double_dkey, decode_float_dkey,
    decode_float_oneof_dkey, decode_int_oneof_dkey, decode_integer_dkey, decode_string_dkey,
    is_dkey_iri,
};
pub use convert_back::{axiom_to_component, concept_to_class_expression, convert_back};
pub use data_axioms::{
    DateKey, DateTimeKey, Decimal, OrdF64, StrSet, literal_provably_outside_range,
};
pub use definitions::{Definitions, extract_definitions};
pub use ir::{ClassId, ConceptExpr, ConceptId, ConceptPool, IndividualId, Role, RoleId};
pub use normalize::{is_nnf, nnf_axioms, nnf_complement, to_nnf};
pub use ontology::{Axiom, InternalOntology, SubRolePath};
pub use render::{debug_render_axiom, render_concept};
pub use role_hierarchy::{RoleHierarchy, RoleHierarchyBuilder};
pub use told::{ToldTables, build_told_tables};
pub use transform::transform;
pub use vocab::Vocabulary;
