//! The semantics of `owl:bottomObjectProperty` / `owl:bottomDataProperty`, and the
//! inconsistencies a universal role makes, stated as axioms at conversion (#539).
//!
//! The IR interns the four built-in property IRIs as ordinary roles, so no engine knew
//! that the bottom roles are empty: `C ⊑ ∃owl:bottomObjectProperty.D` classified `C`
//! satisfiable with `completeness_guaranteed: true`, and `r ⊑ owl:bottomObjectProperty`
//! with `r(a, b)` read consistent, so realize and instance gave a definite wrong "no".
//!
//! Conversion now adds axioms that hold in every interpretation of the OWL 2 Direct
//! Semantics, so no false positive is possible, and every engine applies them through its
//! existing role-hierarchy machinery (the EL saturator's ∃-on-the-left rule, the `ABox`
//! saturation and pre-checks, the tableau, the wedge's clauses):
//!
//! - **A1/A2.** `∃b.⊤ ⊑ ⊥` for `b` = `owl:bottomObjectProperty` and for
//!   `b` = `owl:bottomDataProperty`, each when the role is interned (some axiom mentions
//!   it). `∃p.rdfs:Literal` lowers to `∃p.⊤` (#314), so one shape serves both. Through
//!   the role hierarchy this empties every `r ⊑* b`: chains, inverses and equivalences
//!   included (55 probe shapes, #539's Fable plan review).
//! - **A3.** `⊤ ⊑ ⊥` when the universal object role is told empty: some role is both
//!   universal (`owl:topObjectProperty`, or above it, through inclusion, equivalence,
//!   inverses and chains whose legs are all universal) and empty (a bottom role, or below
//!   one, through inclusion, equivalence and inverses). A universal role relates every
//!   element of the non-empty domain to itself, so it cannot be empty.
//!   `owl:topDataProperty` is deliberately NOT a seed: OWL 2 DL allows it only as the
//!   super-property of `SubDataPropertyOf` (the restrictions on the axiom closure), and on
//!   `SubDataPropertyOf(owl:topDataProperty owl:bottomDataProperty)` both `HermiT` and
//!   Konclude answer "consistent", so stating `⊤ ⊑ ⊥` there would be a positive claim
//!   both oracles contradict.
//! - **A4.** `⊤ ⊑ ⊥` when an object property assertion on a universal role is negated:
//!   `¬u(a, b)` contradicts `u = Δ × Δ`.
//!
//! Nothing else about the universal role is implemented: it is otherwise an ordinary role
//! (a sound under-approximation; its signal is #539's PR2, its semantics #543). The added
//! axioms are recorded in [`InternalOntology::builtin`] so that surfaces citing the
//! source's axioms do not present them as the user's.

use std::collections::BTreeSet;

use crate::ir::{Role, RoleId};
use crate::ontology::{Axiom, InternalOntology, SubRolePath};

pub const OWL_TOP_OBJECT_PROPERTY: &str = "http://www.w3.org/2002/07/owl#topObjectProperty";
pub const OWL_BOTTOM_OBJECT_PROPERTY: &str = "http://www.w3.org/2002/07/owl#bottomObjectProperty";
pub const OWL_TOP_DATA_PROPERTY: &str = "http://www.w3.org/2002/07/owl#topDataProperty";
pub const OWL_BOTTOM_DATA_PROPERTY: &str = "http://www.w3.org/2002/07/owl#bottomDataProperty";

/// The roles that are universal: `owl:topObjectProperty` if interned (not
/// `owl:topDataProperty`; see the module doc), closed upward through
/// `SubObjectPropertyOf` (a chain whose legs are all universal counts: `U ∘ U = U` on a
/// non-empty domain), `EquivalentObjectProperties` and `InverseObjectProperties`
/// (`U⁻ = U`, so polarity is irrelevant and the set is over role ids).
fn universal_roles(out: &InternalOntology) -> BTreeSet<RoleId> {
    let mut u: BTreeSet<RoleId> = out
        .vocabulary
        .role_id(OWL_TOP_OBJECT_PROPERTY)
        .into_iter()
        .collect();
    if u.is_empty() {
        return u;
    }
    loop {
        let before = u.len();
        for ax in &out.axioms {
            match ax {
                Axiom::SubObjectPropertyOf { sub, sup } => {
                    let all_universal = match sub {
                        SubRolePath::Role(r) => u.contains(&r.role_id()),
                        SubRolePath::Chain(legs) => {
                            !legs.is_empty() && legs.iter().all(|r| u.contains(&r.role_id()))
                        }
                    };
                    if all_universal {
                        u.insert(sup.role_id());
                    }
                }
                Axiom::EquivalentObjectProperties(rs)
                    if rs.iter().any(|r| u.contains(&r.role_id())) =>
                {
                    u.extend(rs.iter().map(|r| r.role_id()));
                }
                Axiom::InverseObjectProperties(p, q)
                    if u.contains(&p.role_id()) || u.contains(&q.role_id()) =>
                {
                    u.insert(p.role_id());
                    u.insert(q.role_id());
                }
                _ => {}
            }
        }
        if u.len() == before {
            return u;
        }
    }
}

/// The roles told empty: the interned bottom properties, closed DOWNWARD through
/// `SubObjectPropertyOf` with a single-role sub (a chain into an empty role empties the
/// composition, not its legs), `EquivalentObjectProperties` and
/// `InverseObjectProperties`.
fn empty_roles(out: &InternalOntology) -> BTreeSet<RoleId> {
    let mut b: BTreeSet<RoleId> = [OWL_BOTTOM_OBJECT_PROPERTY, OWL_BOTTOM_DATA_PROPERTY]
        .iter()
        .filter_map(|iri| out.vocabulary.role_id(iri))
        .collect();
    if b.is_empty() {
        return b;
    }
    loop {
        let before = b.len();
        for ax in &out.axioms {
            match ax {
                Axiom::SubObjectPropertyOf {
                    sub: SubRolePath::Role(r),
                    sup,
                } if b.contains(&sup.role_id()) => {
                    b.insert(r.role_id());
                }
                Axiom::EquivalentObjectProperties(rs)
                    if rs.iter().any(|r| b.contains(&r.role_id())) =>
                {
                    b.extend(rs.iter().map(|r| r.role_id()));
                }
                Axiom::InverseObjectProperties(p, q)
                    if b.contains(&p.role_id()) || b.contains(&q.role_id()) =>
                {
                    b.insert(p.role_id());
                    b.insert(q.role_id());
                }
                _ => {}
            }
        }
        if b.len() == before {
            return b;
        }
    }
}

/// Add A1–A4 (see the module doc) to `out.axioms` and record them in `out.builtin`.
/// Interns no role and no class that the source did not already mention, and adds
/// nothing to an ontology without the built-in property IRIs.
pub(crate) fn state_top_bottom_role_semantics(out: &mut InternalOntology) {
    let mut added: Vec<Axiom> = Vec::new();
    // A1/A2.
    for iri in [OWL_BOTTOM_OBJECT_PROPERTY, OWL_BOTTOM_DATA_PROPERTY] {
        if let Some(id) = out.vocabulary.role_id(iri) {
            let top = out.concepts.top();
            let sub = out.concepts.some(Role::named(id), top);
            let sup = out.concepts.bot();
            added.push(Axiom::SubClassOf { sub, sup });
        }
    }
    // A3/A4.
    let universal = universal_roles(out);
    if !universal.is_empty() {
        let told_empty = !empty_roles(out).is_disjoint(&universal);
        let negated = out.axioms.iter().any(|ax| {
            matches!(ax, Axiom::NegativeObjectPropertyAssertion { role, .. }
                if universal.contains(&role.role_id()))
        });
        if told_empty || negated {
            let sub = out.concepts.top();
            let sup = out.concepts.bot();
            added.push(Axiom::SubClassOf { sub, sup });
        }
    }
    // An axiom the source already states (the same interned shape) stays the user's:
    // it is neither duplicated nor recorded as built-in.
    for ax in added {
        if !out.axioms.contains(&ax) && !out.builtin.contains(&ax) {
            out.axioms.push(ax.clone());
            out.builtin.push(ax);
        }
    }
}
