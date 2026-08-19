//! Deterministic debug rendering of the internal IR at the IRI level.
//!
//! Test infrastructure. `ConceptId`s and axiom indices are a function of the
//! WHOLE axiom set (`convert_ontology` sorts the components before interning
//! and sorts the axiom list again afterwards), so two ontologies built by
//! different routes — from scratch versus incrementally — are never id-
//! comparable even when they are semantically identical. Rendering both to
//! IRI-level strings and comparing the sorted multisets is the comparison that
//! actually means something.
//!
//! The output is a stable, total, structural encoding; it is NOT meant to be
//! human-facing syntax (`justify` renders Manchester for that).

use crate::ir::{ConceptExpr, ConceptId, Role};
use crate::ontology::{Axiom, SubRolePath};
use crate::{ConceptPool, Vocabulary};

fn render_role(role: Role, vocab: &Vocabulary) -> String {
    match role {
        Role::Named(id) => vocab.role_iri(id).to_owned(),
        Role::Inverse(id) => format!("inv({})", vocab.role_iri(id)),
    }
}

fn render_path(path: &SubRolePath, vocab: &Vocabulary) -> String {
    match path {
        SubRolePath::Role(r) => render_role(*r, vocab),
        SubRolePath::Chain(rs) => {
            let parts: Vec<String> = rs.iter().map(|r| render_role(*r, vocab)).collect();
            format!("chain({})", parts.join(" o "))
        }
    }
}

fn render_list(ids: &[ConceptId], vocab: &Vocabulary, pool: &ConceptPool) -> String {
    let parts: Vec<String> = ids
        .iter()
        .map(|c| render_concept(*c, vocab, pool))
        .collect();
    parts.join(", ")
}

/// Render a concept expression as a deterministic IRI-level string.
#[must_use]
pub fn render_concept(id: ConceptId, vocab: &Vocabulary, pool: &ConceptPool) -> String {
    match pool.get(id) {
        ConceptExpr::Top => "Top".to_owned(),
        ConceptExpr::Bot => "Bot".to_owned(),
        ConceptExpr::Atomic(c) => vocab.class_iri(*c).to_owned(),
        ConceptExpr::Nominal(i) => format!("{{{}}}", vocab.individual_iri(*i)),
        ConceptExpr::SelfRestriction(r) => format!("Self({})", render_role(*r, vocab)),
        ConceptExpr::Not(c) => format!("Not({})", render_concept(*c, vocab, pool)),
        ConceptExpr::And(cs) => format!("And({})", render_list(cs, vocab, pool)),
        ConceptExpr::Or(cs) => format!("Or({})", render_list(cs, vocab, pool)),
        ConceptExpr::Some(r, c) => format!(
            "Some({}, {})",
            render_role(*r, vocab),
            render_concept(*c, vocab, pool)
        ),
        ConceptExpr::All(r, c) => format!(
            "All({}, {})",
            render_role(*r, vocab),
            render_concept(*c, vocab, pool)
        ),
        ConceptExpr::Min(n, r, c) => format!(
            "Min({n}, {}, {})",
            render_role(*r, vocab),
            render_concept(*c, vocab, pool)
        ),
        ConceptExpr::Max(n, r, c) => format!(
            "Max({n}, {}, {})",
            render_role(*r, vocab),
            render_concept(*c, vocab, pool)
        ),
    }
}

/// Render an axiom as a deterministic IRI-level string.
///
/// Two axioms render equal iff they are structurally equal modulo the id
/// assignment of the pool/vocabulary they were interned into.
#[must_use]
#[allow(clippy::too_many_lines)] // one arm per Axiom variant; a wildcard would silently under-render
pub fn debug_render_axiom(ax: &Axiom, vocab: &Vocabulary, pool: &ConceptPool) -> String {
    let ind = |i: &crate::ir::IndividualId| vocab.individual_iri(*i).to_owned();
    let inds = |is: &[crate::ir::IndividualId]| is.iter().map(ind).collect::<Vec<_>>().join(", ");
    match ax {
        Axiom::SubClassOf { sub, sup } => format!(
            "SubClassOf({}, {})",
            render_concept(*sub, vocab, pool),
            render_concept(*sup, vocab, pool)
        ),
        Axiom::EquivalentClasses(cs) => {
            format!("EquivalentClasses({})", render_list(cs, vocab, pool))
        }
        Axiom::DisjointClasses(cs) => format!("DisjointClasses({})", render_list(cs, vocab, pool)),
        Axiom::DisjointUnion { class, members } => format!(
            "DisjointUnion({}, {})",
            vocab.class_iri(*class),
            render_list(members, vocab, pool)
        ),
        Axiom::SubObjectPropertyOf { sub, sup } => format!(
            "SubObjectPropertyOf({}, {})",
            render_path(sub, vocab),
            render_role(*sup, vocab)
        ),
        Axiom::EquivalentObjectProperties(rs) => format!(
            "EquivalentObjectProperties({})",
            rs.iter()
                .map(|r| render_role(*r, vocab))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Axiom::DisjointObjectProperties(rs) => format!(
            "DisjointObjectProperties({})",
            rs.iter()
                .map(|r| render_role(*r, vocab))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Axiom::InverseObjectProperties(a, b) => format!(
            "InverseObjectProperties({}, {})",
            render_role(*a, vocab),
            render_role(*b, vocab)
        ),
        Axiom::ObjectPropertyDomain { role, domain } => format!(
            "ObjectPropertyDomain({}, {})",
            render_role(*role, vocab),
            render_concept(*domain, vocab, pool)
        ),
        Axiom::ObjectPropertyRange { role, range } => format!(
            "ObjectPropertyRange({}, {})",
            render_role(*role, vocab),
            render_concept(*range, vocab, pool)
        ),
        Axiom::TransitiveRole(r) => format!("TransitiveRole({})", render_role(*r, vocab)),
        Axiom::SymmetricRole(r) => format!("SymmetricRole({})", render_role(*r, vocab)),
        Axiom::AsymmetricRole(r) => format!("AsymmetricRole({})", render_role(*r, vocab)),
        Axiom::ReflexiveRole(r) => format!("ReflexiveRole({})", render_role(*r, vocab)),
        Axiom::IrreflexiveRole(r) => format!("IrreflexiveRole({})", render_role(*r, vocab)),
        Axiom::FunctionalRole(r) => format!("FunctionalRole({})", render_role(*r, vocab)),
        Axiom::InverseFunctionalRole(r) => {
            format!("InverseFunctionalRole({})", render_role(*r, vocab))
        }
        Axiom::ClassAssertion { class, individual } => format!(
            "ClassAssertion({}, {})",
            render_concept(*class, vocab, pool),
            ind(individual)
        ),
        Axiom::ObjectPropertyAssertion {
            role,
            subject,
            object,
        } => format!(
            "ObjectPropertyAssertion({}, {}, {})",
            render_role(*role, vocab),
            ind(subject),
            ind(object)
        ),
        Axiom::NegativeObjectPropertyAssertion {
            role,
            subject,
            object,
        } => format!(
            "NegativeObjectPropertyAssertion({}, {}, {})",
            render_role(*role, vocab),
            ind(subject),
            ind(object)
        ),
        Axiom::SameIndividual(is) => format!("SameIndividual({})", inds(is)),
        Axiom::DifferentIndividuals(is) => format!("DifferentIndividuals({})", inds(is)),
        Axiom::DeclareClass(c) => format!("DeclareClass({})", vocab.class_iri(*c)),
        Axiom::DeclareObjectProperty(r) => {
            format!("DeclareObjectProperty({})", vocab.role_iri(*r))
        }
        Axiom::DeclareNamedIndividual(i) => {
            format!("DeclareNamedIndividual({})", vocab.individual_iri(*i))
        }
    }
}
