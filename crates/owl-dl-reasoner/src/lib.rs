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

use std::collections::HashMap;

use horned_owl::model::ForIRI;
use horned_owl::ontology::set::SetOntology;
use thiserror::Error;

use owl_dl_core::convert::{ConversionError, convert_ontology};
use owl_dl_core::{
    AbsorbedTBox, Axiom, ClassId, ConceptExpr, ConceptId, ConceptPool, IndividualId,
    InternalOntology, RoleHierarchy, RoleHierarchyBuilder, RoleId, SubRolePath, absorb, nnf_axioms,
    nnf_complement,
};
use owl_dl_tableau::{NodeId, TableauContext};

/// Recursion depth cap for the search driver — generous and
/// defensive. Real ALCHIQ inputs terminate via pair blocking long
/// before this matters.
const MAX_SEARCH_DEPTH: usize = 256;

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
    let normalized = nnf_axioms(&mut internal);
    let tbox = absorb(&normalized, &mut internal.concepts);
    // Ensure `⊥` is interned — `apply_max` flags inequality clashes
    // by adding `Bot` to the offending node's label set, and looks
    // up the canonical id via `pool.bot_id()`. Cheap & idempotent.
    let _ = internal.concepts.bot();
    let complements = precompute_max_complements(&mut internal.concepts);
    let abox = collect_abox(&mut internal);
    decide(
        &internal.concepts,
        &tbox,
        &hierarchy,
        &inverse_pairs,
        &complements,
        &abox,
        class_id,
    )
}

/// Pre-resolved `ABox` state, ready to seed into the tableau context.
/// All `ConceptId` fields are interned in the pool by
/// [`collect_abox`] (the last stage to mutate the pool); the tableau
/// then runs with a frozen pool.
#[derive(Default, Debug)]
struct Abox {
    /// `(individual, Nominal(individual)_id)` — one entry per
    /// individual referenced in any `ABox` axiom. Each gets a root
    /// node seeded with the nominal label before the test class is
    /// added.
    individuals: Vec<(IndividualId, ConceptId)>,
    /// `(individual, class_concept_id)` from `ClassAssertion`.
    class_assertions: Vec<(IndividualId, ConceptId)>,
    /// `(from_individual, role_id, to_individual)` from
    /// `ObjectPropertyAssertion`. Role polarity has been normalized:
    /// an inverse-role assertion swaps subject/object so the role
    /// stored here is always forward.
    property_assertions: Vec<(IndividualId, RoleId, IndividualId)>,
    /// `(individual, ∀r.¬{b}_concept_id)` from
    /// `NegativeObjectPropertyAssertion`. Encoded as a label that
    /// will be propagated by `apply_forall` along any matching
    /// edge — any actual r-relation to `b`'s nominal causes a
    /// `Not(Nominal(b))` / `Nominal(b)` clash.
    negative_property_assertions: Vec<(IndividualId, ConceptId)>,
    /// `(a, b)` pairs from `SameIndividual(a, b, ...)`. Decomposed
    /// pairwise — the tableau merges `b` into `a` for each pair.
    same_pairs: Vec<(IndividualId, IndividualId)>,
    /// `(a, b)` pairs from `DifferentIndividuals(a, b, ...)`.
    /// Likewise pairwise; the tableau marks them distinct.
    different_pairs: Vec<(IndividualId, IndividualId)>,
}

fn collect_abox(internal: &mut InternalOntology) -> Abox {
    use std::collections::HashSet;
    let mut abox = Abox::default();
    let mut seen: HashSet<IndividualId> = HashSet::new();
    let record_individual = |ind: IndividualId,
                             pool: &mut ConceptPool,
                             seen: &mut HashSet<IndividualId>,
                             abox: &mut Abox| {
        if seen.insert(ind) {
            let nom = pool.nominal(ind);
            abox.individuals.push((ind, nom));
        }
    };
    // First pass: enumerate every individual referenced and intern
    // its Nominal expression.
    for ax in &internal.axioms {
        match ax {
            Axiom::ClassAssertion { individual, .. } => {
                record_individual(*individual, &mut internal.concepts, &mut seen, &mut abox);
            }
            Axiom::ObjectPropertyAssertion {
                subject, object, ..
            }
            | Axiom::NegativeObjectPropertyAssertion {
                subject, object, ..
            } => {
                record_individual(*subject, &mut internal.concepts, &mut seen, &mut abox);
                record_individual(*object, &mut internal.concepts, &mut seen, &mut abox);
            }
            Axiom::SameIndividual(inds) | Axiom::DifferentIndividuals(inds) => {
                for ind in inds {
                    record_individual(*ind, &mut internal.concepts, &mut seen, &mut abox);
                }
            }
            _ => {}
        }
    }
    // Second pass: derive concrete assertions / clashes / pairs.
    // We collect axiom references in a local Vec to avoid double-
    // borrowing internal during the body.
    let axioms: Vec<Axiom> = internal.axioms.clone();
    for ax in &axioms {
        match ax {
            Axiom::ClassAssertion { class, individual } => {
                abox.class_assertions.push((*individual, *class));
            }
            Axiom::ObjectPropertyAssertion {
                role,
                subject,
                object,
            } => {
                let (from, to) = if role.is_inverse() {
                    (*object, *subject)
                } else {
                    (*subject, *object)
                };
                abox.property_assertions.push((from, role.role_id(), to));
            }
            Axiom::NegativeObjectPropertyAssertion {
                role,
                subject,
                object,
            } => {
                // Encode `(subject, object) ∉ role` as
                // `{subject} ⊑ ∀role.¬{object}`. Polarity of the
                // role passes through unchanged.
                let nom_b = internal.concepts.nominal(*object);
                let not_nom_b = internal.concepts.not(nom_b);
                let forall = internal.concepts.all(*role, not_nom_b);
                abox.negative_property_assertions.push((*subject, forall));
            }
            Axiom::SameIndividual(inds) => {
                for i in 0..inds.len() {
                    for j in (i + 1)..inds.len() {
                        abox.same_pairs.push((inds[i], inds[j]));
                    }
                }
            }
            Axiom::DifferentIndividuals(inds) => {
                for i in 0..inds.len() {
                    for j in (i + 1)..inds.len() {
                        abox.different_pairs.push((inds[i], inds[j]));
                    }
                }
            }
            _ => {}
        }
    }
    abox
}

/// Pre-compute NNF complements for every body appearing in a
/// `Max(_, _, body)` expression so the choose rule can look them up
/// without mutating the pool at tableau time. This is the last
/// stage that mutates the pool; after this call the pool is frozen
/// for the tableau run.
fn precompute_max_complements(pool: &mut ConceptPool) -> Vec<(ConceptId, ConceptId)> {
    // Two-step to avoid borrowing the pool both mutably and
    // immutably: collect bodies first, then intern complements.
    let bodies: Vec<ConceptId> = pool
        .iter_with_ids()
        .filter_map(|(_, e)| match e {
            ConceptExpr::Max(_, _, body) => Some(*body),
            _ => None,
        })
        .collect();
    let mut out = Vec::with_capacity(bodies.len());
    for body in bodies {
        let neg = nnf_complement(body, pool);
        out.push((body, neg));
    }
    out
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

#[allow(clippy::too_many_arguments)]
fn decide(
    pool: &ConceptPool,
    tbox: &AbsorbedTBox,
    hierarchy: &RoleHierarchy,
    inverse_pairs: &[(RoleId, RoleId)],
    complements: &[(ConceptId, ConceptId)],
    abox: &Abox,
    class_id: ClassId,
) -> Result<bool, ReasonError> {
    let mut pool = pool.clone();
    let test_concept: ConceptId = pool.atomic(class_id);
    let mut ctx = TableauContext::with_tbox_and_hierarchy(&pool, tbox, hierarchy);
    for &(r, s) in inverse_pairs {
        ctx.declare_inverse_pair(r, s);
    }
    for &(body, comp) in complements {
        ctx.set_complement(body, comp);
    }

    // Phase 5 `ABox` seeding. Order matters:
    // 1. Create a nominal root for each individual.
    // 2. DifferentIndividuals — mark before any merges so a later
    //    SameIndividual on the same pair is detected as a clash.
    // 3. SameIndividual merges; failed merges (declared distinct)
    //    flag the surviving node with ⊥.
    // 4. ClassAssertion / NegativeObjectPropertyAssertion labels.
    // 5. ObjectPropertyAssertion edges between nominal roots.
    // Then add the test class to a fresh anonymous root and run.
    let mut roots: HashMap<IndividualId, NodeId> = HashMap::new();
    for &(ind, nom) in &abox.individuals {
        let node = ctx.new_node();
        ctx.add_label(node, nom);
        ctx.assign_nominal(ind, node);
        roots.insert(ind, node);
    }
    for &(left, right) in &abox.different_pairs {
        if let (Some(&nleft), Some(&nright)) = (roots.get(&left), roots.get(&right)) {
            let nleft = ctx.resolve(nleft);
            let nright = ctx.resolve(nright);
            ctx.mark_distinct(nleft, nright);
        }
    }
    for &(left, right) in &abox.same_pairs {
        if let (Some(&nleft), Some(&nright)) = (roots.get(&left), roots.get(&right)) {
            let target = ctx.resolve(nleft);
            let source = ctx.resolve(nright);
            if target == source {
                continue;
            }
            if !ctx.merge_into(source, target)
                && let Some(bot) = ctx.pool().bot_id()
            {
                ctx.add_label(target, bot);
            }
        }
    }
    for &(ind, c) in &abox.class_assertions {
        if let Some(&n) = roots.get(&ind) {
            let target = ctx.resolve(n);
            ctx.add_label(target, c);
        }
    }
    for &(ind, c) in &abox.negative_property_assertions {
        if let Some(&n) = roots.get(&ind) {
            let target = ctx.resolve(n);
            ctx.add_label(target, c);
        }
    }
    for &(from, role, to) in &abox.property_assertions {
        if let (Some(&nf), Some(&nt)) = (roots.get(&from), roots.get(&to)) {
            let from_n = ctx.resolve(nf);
            let to_n = ctx.resolve(nt);
            ctx.add_edge(from_n, role, to_n);
        }
    }

    // Now the test class on a fresh anonymous root.
    let test_root = ctx.new_node();
    ctx.add_label(test_root, test_concept);

    owl_dl_tableau::search(&mut ctx, MAX_SEARCH_DEPTH).ok_or(ReasonError::NoVerdict)
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
    fn abox_class_assertion_propagates_to_nominal() {
        // ClassAssertion(A, alice); Test ≡ {alice} ⊓ ¬A — unsat
        // because the `ABox` forces alice into A.
        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(Class(:Test))\n\
    Declaration(NamedIndividual(:alice))\n\
    ClassAssertion(:A :alice)\n\
    EquivalentClasses(:Test ObjectIntersectionOf(\
        ObjectOneOf(:alice) ObjectComplementOf(:A)))\n\
)\n"
        ));
        assert!(!check(&onto, "http://rustdl.test/Test"));
    }

    #[test]
    fn abox_same_and_different_is_inconsistent() {
        // SameIndividual + DifferentIndividuals on the same pair —
        // the ontology has no model. Any class query should be unsat.
        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:Test))\n\
    Declaration(NamedIndividual(:alice))\n\
    Declaration(NamedIndividual(:bob))\n\
    DifferentIndividuals(:alice :bob)\n\
    SameIndividual(:alice :bob)\n\
    EquivalentClasses(:Test ObjectOneOf(:alice))\n\
)\n"
        ));
        assert!(!check(&onto, "http://rustdl.test/Test"));
    }

    #[test]
    fn nominal_forces_witness_merge() {
        // ∃r.(A ⊓ {alice}) ⊓ ∃r.(B ⊓ {alice}) — the two existentials
        // generate separate witnesses, but both carry {alice}; the
        // nominal-assignment rule merges them into one node carrying
        // A and B. Satisfiable.
        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(Class(:B))\n\
    Declaration(Class(:Test))\n\
    Declaration(ObjectProperty(:r))\n\
    Declaration(NamedIndividual(:alice))\n\
    SubClassOf(:Test ObjectIntersectionOf(\
        ObjectSomeValuesFrom(:r ObjectIntersectionOf(:A ObjectOneOf(:alice))) \
        ObjectSomeValuesFrom(:r ObjectIntersectionOf(:B ObjectOneOf(:alice)))))\n\
)\n"
        ));
        assert!(check(&onto, "http://rustdl.test/Test"));
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
    fn max_cardinality_alone_is_satisfiable() {
        // ≤1 r.A alone is trivially satisfiable — pick a model with
        // zero or one r-successors. Tests that Max parses, lowers,
        // and saturates without error.
        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(Class(:Test))\n\
    Declaration(ObjectProperty(:r))\n\
    SubClassOf(:Test ObjectMaxCardinality(1 :r :A))\n\
)\n"
        ));
        assert!(check(&onto, "http://rustdl.test/Test"));
    }

    #[test]
    fn min_and_max_conflict_unsat() {
        // ≥2 r.A ⊓ ≤1 r.A — two distinct A-witnesses required, only
        // one allowed. The merge rule cannot collapse them
        // (apply_min marked them distinct); inequality clash.
        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(Class(:Test))\n\
    Declaration(ObjectProperty(:r))\n\
    SubClassOf(:Test ObjectIntersectionOf(\
        ObjectMinCardinality(2 :r :A) \
        ObjectMaxCardinality(1 :r :A)))\n\
)\n"
        ));
        assert!(!check(&onto, "http://rustdl.test/Test"));
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
