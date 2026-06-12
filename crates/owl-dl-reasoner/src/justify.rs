//! Black-box justification: minimal responsible-axiom sets for an entailment,
//! found by re-checking subsets of the ontology's axioms via the public
//! reasoner API. No engine internals.

use horned_owl::model::{
    Build, ClassExpression, Component, EquivalentClasses, ForIRI, MutableOntology,
};
use horned_owl::ontology::set::SetOntology;

use crate::ReasonError;

/// An entailment to justify ("why does this hold?").
#[derive(Debug, Clone)]
pub enum Entailment {
    SubClassOf { sub: String, sup: String },
    EquivalentClasses { a: String, b: String },
    DisjointClasses { a: String, b: String },
    Unsatisfiable { class: String },
    InstanceOf { individual: String, class: String },
    Inconsistent,
}

const PROBE_IRI: &str = "urn:rustdl-justify-probe";

/// Does `onto` entail `q`? Reduces to the public reasoner checks. The
/// `DisjointClasses` case injects a fresh probe class `X ≡ a ⊓ b` and checks
/// `X` unsatisfiable (probe = query encoding; never part of a justification).
///
/// # Errors
/// Propagates [`ReasonError`] from the underlying reasoner.
pub fn entails<A: ForIRI>(onto: &SetOntology<A>, q: &Entailment) -> Result<bool, ReasonError> {
    match q {
        Entailment::SubClassOf { sub, sup } => crate::is_subclass_of(onto, sub, sup),
        Entailment::EquivalentClasses { a, b } => {
            Ok(crate::is_subclass_of(onto, a, b)? && crate::is_subclass_of(onto, b, a)?)
        }
        Entailment::DisjointClasses { a, b } => {
            let mut probed = onto.clone();
            let build: Build<A> = Build::new();
            probed.insert(Component::EquivalentClasses(EquivalentClasses(vec![
                ClassExpression::Class(build.class(PROBE_IRI)),
                ClassExpression::ObjectIntersectionOf(vec![
                    ClassExpression::Class(build.class(a.as_str())),
                    ClassExpression::Class(build.class(b.as_str())),
                ]),
            ])));
            Ok(!crate::is_class_satisfiable(&probed, PROBE_IRI)?)
        }
        Entailment::Unsatisfiable { class } => Ok(!crate::is_class_satisfiable(onto, class)?),
        // is_instance_of is (class, individual) — class first.
        Entailment::InstanceOf { individual, class } => {
            crate::is_instance_of(onto, class, individual)
        }
        Entailment::Inconsistent => Ok(!crate::is_consistent(onto)?),
    }
}
