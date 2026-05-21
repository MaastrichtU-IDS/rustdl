//! Hybrid saturation+tableau OWL DL reasoner — the public API surface.
//!
//! End-users depend on this crate. Internally it orchestrates `owl-dl-core`
//! (IR, preprocessing), `owl-dl-saturation` (EL fragment),
//! `owl-dl-tableau` (SROIQ), and `owl-dl-datatypes` (concrete domains).
//!
//! ## Scope today
//!
//! As of Phase 2 commit 6 the only public entry point is
//! [`is_class_satisfiable`]: given a parsed horned-owl ontology and a
//! class IRI, run the full normalization+absorption+tableau pipeline
//! and answer "is this class non-empty in some model of the
//! ontology?". Limited to pure ALC for now; later phases extend to
//! `ALCHIQ` and full `SROIQ(D)`.

use horned_owl::model::ForIRI;
use horned_owl::ontology::set::SetOntology;
use thiserror::Error;

use owl_dl_core::convert::{ConversionError, convert_ontology};
use owl_dl_core::{
    AbsorbedTBox, ClassId, ConceptId, ConceptPool, InternalOntology, absorb, nnf_axioms,
};
use owl_dl_tableau::TableauContext;

/// Errors that can surface from the public reasoning API.
#[derive(Debug, Error)]
pub enum ReasonError {
    /// horned-owl axioms couldn't be lowered to the internal IR.
    /// Most often: a construct rustdl doesn't support yet (inverse
    /// roles, data ranges, anonymous individuals, ...).
    #[error("conversion from horned-owl: {0}")]
    Conversion(#[from] ConversionError),

    /// The IRI given to [`is_class_satisfiable`] was not declared as
    /// a class in the input ontology. Most often a typo or a missing
    /// `Declaration(Class(...))`.
    #[error("class IRI not in ontology: {0}")]
    UnknownClass(String),

    /// The tableau hit its internal iteration/recursion cap. Should
    /// not happen for inputs in the implemented fragment; bug
    /// indicator.
    #[error("tableau bailed out without a verdict (likely an internal limit)")]
    NoVerdict,
}

/// Decide whether `class_iri` is satisfiable in the ontology.
///
/// Pipeline:
/// 1. Lower horned-owl axioms to the internal IR ([`convert_ontology`]).
/// 2. Push every concept to NNF ([`nnf_axioms`]).
/// 3. Run binary, nominal and role absorption ([`absorb`]).
/// 4. Build a [`TableauContext`] backed by the absorbed `TBox`.
/// 5. Add `Atomic(class)` to a fresh root node and call
///    [`TableauContext::is_satisfiable`].
///
/// Returns `Ok(true)` if `class_iri` is satisfiable w.r.t. the
/// ontology, `Ok(false)` if unsatisfiable, and a [`ReasonError`]
/// otherwise.
///
/// # Errors
///
/// See [`ReasonError`] variants. The most common cause is the IRI
/// not appearing as a declared class in the ontology.
pub fn is_class_satisfiable<A: ForIRI>(
    ontology: &SetOntology<A>,
    class_iri: &str,
) -> Result<bool, ReasonError> {
    let internal = convert_ontology(ontology)?;
    is_class_satisfiable_internal(internal, class_iri)
}

/// Internal entry point that takes the already-lowered ontology.
/// Exposed for tests that want to assemble an `InternalOntology` by
/// hand or share one across multiple satisfiability checks.
///
/// # Errors
///
/// See [`ReasonError`].
pub fn is_class_satisfiable_internal(
    mut internal: InternalOntology,
    class_iri: &str,
) -> Result<bool, ReasonError> {
    let Some(class_id) = internal.vocabulary.class_id(class_iri) else {
        return Err(ReasonError::UnknownClass(class_iri.to_owned()));
    };

    let normalized = nnf_axioms(&mut internal);
    let tbox = absorb(&normalized, &mut internal.concepts);
    decide(&internal.concepts, &tbox, class_id)
}

fn decide(pool: &ConceptPool, tbox: &AbsorbedTBox, class_id: ClassId) -> Result<bool, ReasonError> {
    // The Atomic-of-class concept; interning is cheap and idempotent
    // — if the class appears anywhere in the TBox the id is already
    // in the pool, otherwise this just registers it.
    let mut pool = pool.clone();
    let concept: ConceptId = pool.atomic(class_id);
    let mut ctx = TableauContext::with_tbox(&pool, tbox);
    ctx.is_satisfiable(concept).ok_or(ReasonError::NoVerdict)
}

#[cfg(test)]
mod tests {
    use super::*;
    use horned_owl::io::ParserConfiguration;
    use horned_owl::io::ofn::reader::read;
    use horned_owl::model::RcStr;
    use horned_owl::ontology::set::SetOntology;
    use std::io::Cursor;

    fn parse(src: &str) -> SetOntology<RcStr> {
        let mut reader = Cursor::new(src);
        let (ontology, _prefixes) =
            read(&mut reader, ParserConfiguration::default()).expect("fixture parses");
        ontology
    }

    const HEADER: &str = "\
Prefix(:=<http://rustdl.test/>)\n\
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)\n";

    fn check(onto: &SetOntology<RcStr>, iri: &str) -> bool {
        is_class_satisfiable(onto, iri).expect("verdict returned")
    }

    #[test]
    fn satisfiable_atomic_class() {
        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
)\n"
        ));
        assert!(check(&onto, "http://rustdl.test/A"));
    }

    #[test]
    fn unsatisfiable_via_equivalence() {
        // Test ≡ A ⊓ ¬A — :Test must be unsat.
        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(Class(:Test))\n\
    EquivalentClasses(:Test ObjectIntersectionOf(:A ObjectComplementOf(:A)))\n\
)\n"
        ));
        assert!(!check(&onto, "http://rustdl.test/Test"));
    }

    #[test]
    fn unsatisfiable_via_subsumption_chain() {
        // A ⊑ B, B ⊑ C, Test ≡ A ⊓ ¬C — :Test must be unsat.
        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(Class(:B))\n\
    Declaration(Class(:C))\n\
    Declaration(Class(:Test))\n\
    SubClassOf(:A :B)\n\
    SubClassOf(:B :C)\n\
    EquivalentClasses(:Test ObjectIntersectionOf(:A ObjectComplementOf(:C)))\n\
)\n"
        ));
        assert!(!check(&onto, "http://rustdl.test/Test"));
    }

    #[test]
    fn cyclic_tbox_terminates_via_blocking() {
        // A ⊑ ∃r.A — :A is satisfiable; must terminate.
        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(ObjectProperty(:r))\n\
    SubClassOf(:A ObjectSomeValuesFrom(:r :A))\n\
)\n"
        ));
        assert!(check(&onto, "http://rustdl.test/A"));
    }

    #[test]
    fn unknown_class_iri_errors() {
        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
)\n"
        ));
        let err = is_class_satisfiable(&onto, "http://rustdl.test/Nope")
            .expect_err("unknown class should error");
        assert!(matches!(err, ReasonError::UnknownClass(_)));
    }
}
