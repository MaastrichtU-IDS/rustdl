//! Live signature: which named entities are still mentioned by a LIVE axiom.
//!
//! Ids are append-only and never recycled (`vocab.rs:24-33`), so a session that
//! deletes the last axiom mentioning a class would keep reporting that class
//! forever unless reporting is filtered through this set. See spec §4a.
//!
//! The signature is **recomputed** from the live axiom set rather than
//! maintained as per-entity refcounts: same observable contract, no
//! incrementally-maintained counter that can drift out of sync, at the cost of
//! one O(live axioms) pass.

use fixedbitset::FixedBitSet;

use crate::ir::{ClassId, ConceptExpr, ConceptId, IndividualId, Role, RoleId};
use crate::ontology::SubRolePath;
use crate::{Axiom, ConceptPool, InternalOntology};

/// The set of named entities mentioned by at least one live axiom.
///
/// Bit `i` is set iff the entity with index `i` is still referenced. Bits are
/// only ever *hidden*, never recycled: an id that drops out here still
/// resolves in the vocabulary.
#[derive(Debug, Clone, Default)]
pub struct LiveSignature {
    pub classes: FixedBitSet,
    pub roles: FixedBitSet,
    pub individuals: FixedBitSet,
}

impl LiveSignature {
    #[must_use]
    pub fn has_class(&self, c: ClassId) -> bool {
        let i = c.index() as usize;
        i < self.classes.len() && self.classes.contains(i)
    }

    #[must_use]
    pub fn has_role(&self, r: RoleId) -> bool {
        let i = r.index() as usize;
        i < self.roles.len() && self.roles.contains(i)
    }

    #[must_use]
    pub fn has_individual(&self, ind: IndividualId) -> bool {
        let i = ind.index() as usize;
        i < self.individuals.len() && self.individuals.contains(i)
    }

    fn mark_class(&mut self, c: ClassId) {
        let i = c.index() as usize;
        if i < self.classes.len() {
            self.classes.insert(i);
        }
    }

    fn mark_role_id(&mut self, r: RoleId) {
        let i = r.index() as usize;
        if i < self.roles.len() {
            self.roles.insert(i);
        }
    }

    fn mark_role(&mut self, r: Role) {
        self.mark_role_id(r.role_id());
    }

    fn mark_individual(&mut self, ind: IndividualId) {
        let i = ind.index() as usize;
        if i < self.individuals.len() {
            self.individuals.insert(i);
        }
    }
}

/// Walk every LIVE axiom and mark the entities it mentions.
#[must_use]
pub fn compute(o: &InternalOntology) -> LiveSignature {
    let mut sig = LiveSignature {
        classes: FixedBitSet::with_capacity(o.vocabulary.num_classes()),
        roles: FixedBitSet::with_capacity(o.vocabulary.num_roles()),
        individuals: FixedBitSet::with_capacity(o.vocabulary.num_individuals()),
    };
    for (_idx, ax) in o.live_axioms() {
        mark_axiom(ax, &o.concepts, &mut sig);
    }
    sig
}

fn mark_concept(c: ConceptId, pool: &ConceptPool, sig: &mut LiveSignature) {
    // Iterative walk; the pool is a DAG and concepts can be deeply nested.
    let mut stack = vec![c];
    while let Some(cur) = stack.pop() {
        let expr = pool.get(cur);
        if let ConceptExpr::Atomic(cid) = expr {
            sig.mark_class(*cid);
        }
        stack.extend(expr.child_concepts());
        for r in expr.child_roles() {
            sig.mark_role_id(r);
        }
        for ind in expr.child_individuals() {
            sig.mark_individual(ind);
        }
    }
}

fn mark_role_path(path: &SubRolePath, sig: &mut LiveSignature) {
    match path {
        SubRolePath::Role(r) => sig.mark_role(*r),
        SubRolePath::Chain(rs) => {
            for r in rs {
                sig.mark_role(*r);
            }
        }
    }
}

/// Exhaustive by design: **no wildcard arm**. A new `Axiom` variant must break
/// the build here rather than go silently unvisited - see
/// [`ConceptExpr::child_concepts`].
fn mark_axiom(ax: &Axiom, pool: &ConceptPool, sig: &mut LiveSignature) {
    match ax {
        // --- TBox ---
        Axiom::SubClassOf { sub, sup } => {
            mark_concept(*sub, pool, sig);
            mark_concept(*sup, pool, sig);
        }
        Axiom::EquivalentClasses(cs) | Axiom::DisjointClasses(cs) => {
            for c in cs {
                mark_concept(*c, pool, sig);
            }
        }
        Axiom::DisjointUnion { class, members } => {
            sig.mark_class(*class);
            for c in members {
                mark_concept(*c, pool, sig);
            }
        }

        // --- RBox ---
        Axiom::SubObjectPropertyOf { sub, sup } => {
            mark_role_path(sub, sig);
            sig.mark_role(*sup);
        }
        Axiom::EquivalentObjectProperties(rs) | Axiom::DisjointObjectProperties(rs) => {
            for r in rs {
                sig.mark_role(*r);
            }
        }
        Axiom::InverseObjectProperties(a, b) => {
            sig.mark_role(*a);
            sig.mark_role(*b);
        }
        Axiom::ObjectPropertyDomain { role, domain: c }
        | Axiom::ObjectPropertyRange { role, range: c } => {
            sig.mark_role(*role);
            mark_concept(*c, pool, sig);
        }
        Axiom::TransitiveRole(r)
        | Axiom::SymmetricRole(r)
        | Axiom::AsymmetricRole(r)
        | Axiom::ReflexiveRole(r)
        | Axiom::IrreflexiveRole(r)
        | Axiom::FunctionalRole(r)
        | Axiom::InverseFunctionalRole(r) => sig.mark_role(*r),

        // --- ABox ---
        Axiom::ClassAssertion { class, individual } => {
            mark_concept(*class, pool, sig);
            sig.mark_individual(*individual);
        }
        Axiom::ObjectPropertyAssertion {
            role,
            subject,
            object,
        }
        | Axiom::NegativeObjectPropertyAssertion {
            role,
            subject,
            object,
        } => {
            sig.mark_role(*role);
            sig.mark_individual(*subject);
            sig.mark_individual(*object);
        }
        Axiom::SameIndividual(is) | Axiom::DifferentIndividuals(is) => {
            for i in is {
                sig.mark_individual(*i);
            }
        }

        // --- Declarations ---
        // Spec §4a: a declaration IS a reference. It is what keeps an
        // otherwise-unused entity reportable, and it is how punning stays
        // correct (the same IRI declared as class and role marks both).
        Axiom::DeclareClass(c) => sig.mark_class(*c),
        Axiom::DeclareObjectProperty(r) => sig.mark_role_id(*r),
        Axiom::DeclareNamedIndividual(i) => sig.mark_individual(*i),
    }
}
