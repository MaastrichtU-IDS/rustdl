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
//! Object and data properties share one role table, and conversion has no punning check.
//! So an IRI used as both an object and a data property (outside OWL 2 DL) is one role
//! here: `SubObjectPropertyOf(owl:topObjectProperty :p)` with
//! `SubDataPropertyOf(:p owl:bottomDataProperty)` yields `⊤ ⊑ ⊥`. That conflation predates
//! this module.
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
    upward_closure(
        out,
        out.vocabulary
            .role_id(OWL_TOP_OBJECT_PROPERTY)
            .into_iter()
            .collect(),
    )
}

/// `seeds` closed upward as [`universal_roles`] describes.
fn upward_closure(out: &InternalOntology, mut u: BTreeSet<RoleId>) -> BTreeSet<RoleId> {
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

/// #543: does the ontology use a universal role where its universal-role semantics,
/// which rustdl does not implement, could change a class-level entailment?
///
/// `owl:topObjectProperty` (and `owl:topDataProperty`, which OWL 2 DL allows only as a
/// super-property) is an ordinary role to every engine. That is sound for positives:
/// a model of the ordinary reading in which every universal role `u` is reinterpreted
/// as `Δ × Δ` still satisfies every *inert* occurrence of `u`, so the ordinary
/// entailments are a subset of the universal ones. An occurrence that is not inert
/// can lose entailments, and then `completeness_guaranteed` and every definite
/// negative must say so (condition 9 of `ClassificationStats::completeness_guaranteed`).
///
/// U is both top properties closed upward (inclusion, all-universal chains,
/// equivalence, inverses). **Inert** occurrences of `u ∈ U`:
/// - `x ⊑ u` (any `x`), and a chain whose super-role is in U;
/// - equivalence and inverse within U (the closure puts both sides in U);
/// - `Transitive(u)`, `Symmetric(u)`, `Reflexive(u)`: true of `Δ × Δ`;
/// - a positive assertion `u(a, b)`: true of `Δ × Δ`;
/// - a negative assertion `¬u(a, b)` on an OBJECT universal role: #539's A4 already
///   states `⊤ ⊑ ⊥` for it.
///
/// `owl:topDataProperty` anywhere but the super position of a property inclusion is a
/// use: OWL 2 DL allows nothing else, and #539 states nothing for it (`topData ⊑
/// bottomData` is inconsistent, as `Δ × D` is not empty).
///
/// Everything else is a **use**: `u` inside any concept (`∃`, `∀`, cardinality,
/// `Self`, `HasValue`, data restrictions on `owl:topDataProperty`), the domain or
/// range of `u`, `Functional`, `InverseFunctional`, `Irreflexive` or `Asymmetric(u)`,
/// `u` in a `DisjointObjectProperties`, and `u` as a leg of a chain into a role
/// outside U. The match below is exhaustive on purpose: a new axiom kind must be
/// classified, not defaulted to inert. #539's built-in axioms are skipped (they state
/// semantics; `∃bottom.⊤ ⊑ ⊥` is not a use even when `top ⊑* bottom` puts bottom in U).
///
/// "Inert" is about CLASS-level entailments. Role-level surfaces (`property-values`,
/// materialized property assertions) miss `r(x, y)` for every `r ∈ U \ {top}` under
/// any occurrence; [`universal_super_roles`] names those roles.
///
/// A pure function of the converted ontology: `O(1)` when neither top property is
/// interned, which is every ontology that does not mention them.
#[must_use]
pub fn universal_role_ordinary(out: &InternalOntology) -> bool {
    let seeds: BTreeSet<RoleId> = [OWL_TOP_OBJECT_PROPERTY, OWL_TOP_DATA_PROPERTY]
        .iter()
        .filter_map(|iri| out.vocabulary.role_id(iri))
        .collect();
    if seeds.is_empty() {
        return false;
    }
    let u = upward_closure(out, seeds);
    // #539's A3/A4 cover the OBJECT universal roles only (`universal_roles`); a role
    // universal only through `owl:topDataProperty` gets no built-in, so nothing is
    // inert on #539's account there.
    let u_obj = universal_roles(out);
    let top_data = out.vocabulary.role_id(OWL_TOP_DATA_PROPERTY);
    let mut seen: std::collections::HashMap<crate::ir::ConceptId, bool> =
        std::collections::HashMap::new();
    let roles = RoleSets {
        u: &u,
        u_obj: &u_obj,
        top_data,
    };
    out.axioms
        .iter()
        .filter(|ax| !out.builtin.contains(ax))
        .any(|ax| axiom_uses(ax, &roles, &mut |c| concept_mentions(out, c, &u, &mut seen)))
}

/// The role sets [`axiom_uses`] classifies against.
struct RoleSets<'a> {
    /// Every universal role (both top properties, closed upward).
    u: &'a BTreeSet<RoleId>,
    /// The object universal roles, which #539's A3/A4 cover.
    u_obj: &'a BTreeSet<RoleId>,
    /// `owl:topDataProperty`, if interned.
    top_data: Option<RoleId>,
}

impl RoleSets<'_> {
    fn in_u(&self, r: Role) -> bool {
        self.u.contains(&r.role_id())
    }
    fn is_top_data(&self, r: Role) -> bool {
        self.top_data == Some(r.role_id())
    }
}

/// The roles that are universal only because they sit above a top property
/// (`top ⊑* r`, `r` not a top property itself). Every named pair is an `r`-edge of
/// the ontology, which the role-level surfaces do not report (#543).
#[must_use]
pub fn universal_super_roles(out: &InternalOntology) -> BTreeSet<RoleId> {
    let seeds: BTreeSet<RoleId> = [OWL_TOP_OBJECT_PROPERTY, OWL_TOP_DATA_PROPERTY]
        .iter()
        .filter_map(|iri| out.vocabulary.role_id(iri))
        .collect();
    if seeds.is_empty() {
        return seeds;
    }
    let mut u = upward_closure(out, seeds.clone());
    u.retain(|r| !seeds.contains(r));
    u
}

fn axiom_uses(
    ax: &Axiom,
    roles: &RoleSets<'_>,
    concept: &mut impl FnMut(crate::ir::ConceptId) -> bool,
) -> bool {
    let in_u = |r: &Role| roles.in_u(*r);
    let top_data = |r: &Role| roles.is_top_data(*r);
    match ax {
        Axiom::SubClassOf { sub, sup } => concept(*sub) || concept(*sup),
        Axiom::EquivalentClasses(cs) | Axiom::DisjointClasses(cs) => cs.iter().any(|c| concept(*c)),
        Axiom::DisjointUnion { members, .. } => members.iter().any(|c| concept(*c)),
        Axiom::SubObjectPropertyOf { sub, sup } => match sub {
            // OWL 2 DL allows `owl:topDataProperty` only as a super-property, and no
            // built-in covers it: `topData ⊑ bottomData` is inconsistent (`Δ × D` is
            // not empty) and read as consistent. Any sub use of it is flagged.
            SubRolePath::Role(r) if top_data(r) => true,
            // `x ⊑ u` is inert; `u ⊑ r` puts `r` in U, so it is inert too.
            SubRolePath::Role(_) => false,
            SubRolePath::Chain(legs) => {
                legs.iter().any(top_data) || (!in_u(sup) && legs.iter().any(in_u))
            }
        },
        // Within U these are inert (the closure takes both sides), but not with
        // `owl:topDataProperty` as an operand, for the reason above.
        Axiom::EquivalentObjectProperties(rs) => rs.iter().any(top_data),
        Axiom::InverseObjectProperties(p, q) => top_data(p) || top_data(q),
        // A negated assertion on an OBJECT universal role is #539's A4 (`⊤ ⊑ ⊥`); on
        // a role universal only through `owl:topDataProperty` (punned) nothing states
        // it, so it is a use.
        Axiom::NegativeObjectPropertyAssertion { role, .. } => {
            in_u(role) && !roles.u_obj.contains(&role.role_id())
        }
        Axiom::DisjointObjectProperties(rs) => rs.iter().any(in_u),
        Axiom::ObjectPropertyDomain { role, domain: c }
        | Axiom::ObjectPropertyRange { role, range: c } => in_u(role) || concept(*c),
        Axiom::AsymmetricRole(r)
        | Axiom::IrreflexiveRole(r)
        | Axiom::FunctionalRole(r)
        | Axiom::InverseFunctionalRole(r) => in_u(r),
        Axiom::ClassAssertion { class, .. } => concept(*class),
        // Inert: `Transitive`, `Symmetric`, `Reflexive` (true of `Δ × Δ`); a positive
        // assertion (true of `Δ × Δ`); the rest name no role.
        Axiom::TransitiveRole(_)
        | Axiom::SymmetricRole(_)
        | Axiom::ReflexiveRole(_)
        | Axiom::ObjectPropertyAssertion { .. }
        | Axiom::SameIndividual(_)
        | Axiom::DifferentIndividuals(_)
        | Axiom::DeclareClass(_)
        | Axiom::DeclareObjectProperty(_)
        | Axiom::DeclareNamedIndividual(_) => false,
    }
}

/// Does concept `c` contain a restriction on a role in `u`? Memoised per concept.
fn concept_mentions(
    out: &InternalOntology,
    c: crate::ir::ConceptId,
    u: &BTreeSet<RoleId>,
    seen: &mut std::collections::HashMap<crate::ir::ConceptId, bool>,
) -> bool {
    use crate::ir::ConceptExpr as E;
    if let Some(&v) = seen.get(&c) {
        return v;
    }
    // Provisional entry: the pool is a DAG, but this keeps a cycle from recursing.
    seen.insert(c, false);
    let v = match out.concepts.get(c) {
        E::Top | E::Bot | E::Atomic(_) | E::Nominal(_) => false,
        E::SelfRestriction(r) => u.contains(&r.role_id()),
        E::Not(x) => concept_mentions(out, *x, u, seen),
        E::And(xs) | E::Or(xs) => {
            let xs: Vec<_> = xs.to_vec();
            xs.into_iter().any(|x| concept_mentions(out, x, u, seen))
        }
        E::Some(r, x) | E::All(r, x) | E::Min(_, r, x) | E::Max(_, r, x) => {
            let (r, x) = (*r, *x);
            u.contains(&r.role_id()) || concept_mentions(out, x, u, seen)
        }
    };
    seen.insert(c, v);
    v
}
