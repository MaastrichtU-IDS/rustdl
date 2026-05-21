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
    AbsorbedTBox, Axiom, ClassId, ConceptExpr, ConceptId, ConceptPool, InternalOntology,
    RoleHierarchy, RoleHierarchyBuilder, RoleId, SubRolePath, absorb, nnf_axioms,
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

    /// A `SubObjectPropertyOf` axiom uses a role chain on its left-hand
    /// side. Chain axioms (`r ∘ s ⊑ t`) require Phase 5 (`SROIQ`)
    /// machinery and are not supported by the current `ALCH` tableau.
    #[error("role chain sub-property axioms are deferred to Phase 5 (SROIQ)")]
    RoleChainUnsupported,

    /// The ontology uses a `≤n R.C` (`ObjectMaxCardinality`,
    /// `ObjectExactCardinality`, or `FunctionalRole`) cardinality
    /// restriction. `≥n` is supported (Phase 3 Q1) but `≤n` requires
    /// successor merging + the choose rule — that's Phase 3 Q2.
    #[error("upper-bound cardinality restrictions (≤n R.C) not yet supported (Phase 3 Q2)")]
    MaxCardinalityUnsupported,
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

    let hierarchy = build_role_hierarchy(&internal)?;
    let inverse_pairs = collect_inverse_pairs(&internal);
    // Reject upper-bound cardinality BEFORE NNF — otherwise the
    // ¬Min → Max rewrite would mask user-written ≥n behind a
    // pool-level Max occurrence and we'd be unable to tell them
    // apart. Walking axiom concepts pre-NNF only sees Max if the
    // user wrote one (ObjectMaxCardinality / ObjectExactCardinality /
    // FunctionalRole-derived).
    if user_wrote_max(&internal) {
        return Err(ReasonError::MaxCardinalityUnsupported);
    }
    let normalized = nnf_axioms(&mut internal);
    let tbox = absorb(&normalized, &mut internal.concepts);
    decide(
        &internal.concepts,
        &tbox,
        &hierarchy,
        &inverse_pairs,
        class_id,
    )
}

/// True iff any concept *reachable from the input axioms* contains a
/// `Max(_, _, _)` shape. The reachability walk is bounded by a
/// visited set so cycles in the pool (impossible by construction
/// today, but defensive) terminate.
fn user_wrote_max(internal: &InternalOntology) -> bool {
    use std::collections::HashSet;
    let pool = &internal.concepts;
    let mut visited: HashSet<ConceptId> = HashSet::new();
    let mut stack: Vec<ConceptId> = Vec::new();
    for ax in &internal.axioms {
        collect_axiom_concepts(ax, &mut stack);
    }
    while let Some(c) = stack.pop() {
        if !visited.insert(c) {
            continue;
        }
        match pool.get(c) {
            ConceptExpr::Max(_, _, _) => return true,
            ConceptExpr::Not(inner)
            | ConceptExpr::Some(_, inner)
            | ConceptExpr::All(_, inner)
            | ConceptExpr::Min(_, _, inner) => stack.push(*inner),
            ConceptExpr::And(args) | ConceptExpr::Or(args) => stack.extend(args.iter().copied()),
            _ => {}
        }
    }
    false
}

fn collect_axiom_concepts(ax: &Axiom, out: &mut Vec<ConceptId>) {
    use Axiom::{
        ClassAssertion, DisjointClasses, DisjointUnion, EquivalentClasses, ObjectPropertyDomain,
        ObjectPropertyRange, SubClassOf,
    };
    match ax {
        SubClassOf { sub, sup } => {
            out.push(*sub);
            out.push(*sup);
        }
        EquivalentClasses(ids) | DisjointClasses(ids) => out.extend(ids.iter().copied()),
        DisjointUnion { members, .. } => out.extend(members.iter().copied()),
        ObjectPropertyDomain { domain, .. } => out.push(*domain),
        ObjectPropertyRange { range, .. } => out.push(*range),
        ClassAssertion { class, .. } => out.push(*class),
        _ => {}
    }
}

/// Build the ALCH role hierarchy from atomic `SubObjectPropertyOf` and
/// `EquivalentObjectProperties` axioms. Chain sub-property axioms are
/// rejected with [`ReasonError::RoleChainUnsupported`] (they require
/// Phase 5).
fn build_role_hierarchy(internal: &InternalOntology) -> Result<RoleHierarchy, ReasonError> {
    let mut builder = RoleHierarchyBuilder::with_roles(
        u32::try_from(internal.vocabulary.num_roles()).expect("vocabulary role count fits in u32"),
    );
    for ax in &internal.axioms {
        match ax {
            Axiom::SubObjectPropertyOf { sub, sup } => match sub {
                SubRolePath::Role(sub_role) => {
                    // Only encode the named-to-named portion of the
                    // sub-role lattice; the inverse axis still hangs
                    // off the polarity-check in `edge_satisfies`. If
                    // either side carries an inverse polarity, we'd
                    // need a Role-keyed hierarchy — defer to a later
                    // commit, but still record the underlying-id
                    // relation so same-polarity sub-role inference
                    // remains correct.
                    builder.add_sub_role(sub_role.role_id(), sup.role_id());
                }
                SubRolePath::Chain(_) => return Err(ReasonError::RoleChainUnsupported),
            },
            Axiom::EquivalentObjectProperties(roles) => {
                // r ≡ s ≡ … expands to pairwise sub-property both ways.
                for a in roles {
                    for b in roles {
                        if a != b {
                            builder.add_sub_role(a.role_id(), b.role_id());
                        }
                    }
                }
            }
            _ => {}
        }
    }
    Ok(builder.build())
}

/// Collect declared inverse-role pairs from `InverseObjectProperties`
/// axioms. Each axiom `InverseObjectProperties(r, s)` contributes one
/// `(r.role_id(), s.role_id())` pair; the tableau context populates
/// the map symmetrically.
fn collect_inverse_pairs(internal: &InternalOntology) -> Vec<(RoleId, RoleId)> {
    let mut pairs = Vec::new();
    for ax in &internal.axioms {
        if let Axiom::InverseObjectProperties(a, b) = ax {
            pairs.push((a.role_id(), b.role_id()));
        }
    }
    pairs
}

fn decide(
    pool: &ConceptPool,
    tbox: &AbsorbedTBox,
    hierarchy: &RoleHierarchy,
    inverse_pairs: &[(RoleId, RoleId)],
    class_id: ClassId,
) -> Result<bool, ReasonError> {
    // The Atomic-of-class concept; interning is cheap and idempotent
    // — if the class appears anywhere in the TBox the id is already
    // in the pool, otherwise this just registers it.
    let mut pool = pool.clone();
    let concept: ConceptId = pool.atomic(class_id);
    let mut ctx = TableauContext::with_tbox_and_hierarchy(&pool, tbox, hierarchy);
    for &(r, s) in inverse_pairs {
        ctx.declare_inverse_pair(r, s);
    }
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
    fn role_hierarchy_makes_concept_unsat() {
        // r ⊑ s; ∃r.A ⊓ ∀s.¬A — the sub-property axiom forces the
        // ¬A from ∀s to land on the r-witness too, producing a clash.
        // Without role hierarchy support this would (wrongly) be sat.
        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(Class(:Test))\n\
    Declaration(ObjectProperty(:r))\n\
    Declaration(ObjectProperty(:s))\n\
    SubObjectPropertyOf(:r :s)\n\
    EquivalentClasses(:Test ObjectIntersectionOf(\
        ObjectSomeValuesFrom(:r :A) \
        ObjectAllValuesFrom(:s ObjectComplementOf(:A))))\n\
)\n"
        ));
        assert!(!check(&onto, "http://rustdl.test/Test"));
    }

    #[test]
    fn inverse_object_properties_declared_inverse_matches() {
        // InverseObjectProperties(r, s); Test ≡ ∃r.A ⊓ ∀s⁻.¬A.
        // The declared pair lets the ∀s⁻ rule propagate ¬A through
        // the r-edge, clashing at the witness.
        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(Class(:Test))\n\
    Declaration(ObjectProperty(:r))\n\
    Declaration(ObjectProperty(:s))\n\
    InverseObjectProperties(:r :s)\n\
    EquivalentClasses(:Test ObjectIntersectionOf(\
        ObjectSomeValuesFrom(:r :A) \
        ObjectAllValuesFrom(ObjectInverseOf(:s) ObjectComplementOf(:A))))\n\
)\n"
        ));
        assert!(!check(&onto, "http://rustdl.test/Test"));
    }

    #[test]
    fn min_cardinality_generates_distinct_witnesses() {
        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(Class(:Test))\n\
    Declaration(ObjectProperty(:r))\n\
    SubClassOf(:Test ObjectMinCardinality(3 :r :A))\n\
)\n"
        ));
        assert!(check(&onto, "http://rustdl.test/Test"));
    }

    #[test]
    fn max_cardinality_rejected_with_clear_error() {
        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(Class(:Test))\n\
    Declaration(ObjectProperty(:r))\n\
    SubClassOf(:Test ObjectMaxCardinality(1 :r :A))\n\
)\n"
        ));
        let err = is_class_satisfiable(&onto, "http://rustdl.test/Test")
            .expect_err("Max should error in Q1");
        assert!(matches!(err, ReasonError::MaxCardinalityUnsupported));
    }

    #[test]
    fn role_chain_axiom_rejected_with_clear_error() {
        // SubObjectPropertyOf(ObjectPropertyChain(r s) t) — chain on
        // the LHS. ALCH (Phase 3) doesn't handle chains; rustdl
        // surfaces a dedicated error pointing at Phase 5.
        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(ObjectProperty(:r))\n\
    Declaration(ObjectProperty(:s))\n\
    Declaration(ObjectProperty(:t))\n\
    SubObjectPropertyOf(ObjectPropertyChain(:r :s) :t)\n\
)\n"
        ));
        let err = is_class_satisfiable(&onto, "http://rustdl.test/A")
            .expect_err("role chain should error");
        assert!(matches!(err, ReasonError::RoleChainUnsupported));
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
