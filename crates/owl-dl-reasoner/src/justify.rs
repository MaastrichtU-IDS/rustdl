//! Black-box justification: minimal responsible-axiom sets for an entailment,
//! found by re-checking subsets of the ontology's axioms via the public
//! reasoner API. No engine internals.

use horned_owl::model::{
    Build, ClassExpression, Component, EquivalentClasses, ForIRI, MutableOntology,
};
use horned_owl::ontology::set::SetOntology;

use crate::ReasonError;
use crate::classify::{FragmentClassification, analyze_fragment};

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

/// Split `onto` into (`fixed`, `candidates`): `fixed` = non-logical axioms
/// (declarations / annotations / metadata) retained in every tested ontology;
/// `candidates` = logical axioms, the only possible justification members.
#[must_use]
pub fn logical_axioms<A: ForIRI>(onto: &SetOntology<A>) -> (Vec<Component<A>>, Vec<Component<A>>) {
    let mut fixed = Vec::new();
    let mut candidates = Vec::new();
    for ac in onto {
        let c = ac.component.clone();
        if is_logical(&c) {
            candidates.push(c);
        } else {
            fixed.push(c);
        }
    }
    (fixed, candidates)
}

/// A logical axiom can affect entailment and may appear in a justification;
/// declarations / annotations / ontology metadata cannot.
fn is_logical<A: ForIRI>(c: &Component<A>) -> bool {
    !matches!(
        c,
        Component::OntologyID(_)
            | Component::DocIRI(_)
            | Component::Import(_)
            | Component::OntologyAnnotation(_)
            | Component::DeclareClass(_)
            | Component::DeclareObjectProperty(_)
            | Component::DeclareAnnotationProperty(_)
            | Component::DeclareDataProperty(_)
            | Component::DeclareNamedIndividual(_)
            | Component::DeclareDatatype(_)
            | Component::AnnotationAssertion(_)
            | Component::SubAnnotationPropertyOf(_)
            | Component::AnnotationPropertyDomain(_)
            | Component::AnnotationPropertyRange(_)
    )
}

/// Build a `SetOntology` from `fixed` + the candidate `subset`.
#[must_use]
pub fn ontology_from<A: ForIRI>(fixed: &[Component<A>], subset: &[Component<A>]) -> SetOntology<A> {
    let mut o = SetOntology::new();
    for c in fixed.iter().chain(subset.iter()) {
        o.insert(c.clone());
    }
    o
}

/// A minimal (on EL/Horn) responsible-axiom set for an entailment.
#[derive(Debug, Clone)]
pub struct Justification<A: ForIRI> {
    pub axioms: Vec<Component<A>>,
    pub fragment: FragmentClassification,
    pub minimal_guaranteed: bool,
}

/// Find ONE justification for `q` in `onto`, or `Ok(None)` if `onto` does not
/// entail `q`. `QuickXplain` over the logical axioms; minimal on EL/Horn
/// (rustdl complete), guaranteed-entailing on SROIQ.
///
/// # Errors
/// Propagates [`ReasonError`].
pub fn find_one_justification<A: ForIRI>(
    onto: &SetOntology<A>,
    q: &Entailment,
) -> Result<Option<Justification<A>>, ReasonError> {
    let (fixed, candidates) = logical_axioms(onto);
    if !entails(&ontology_from(&fixed, &candidates), q)? {
        return Ok(None); // not entailed — nothing to justify
    }
    let core = quickxplain(&fixed, &candidates, q)?;
    let fragment = fragment_of(onto);
    let minimal_guaranteed = matches!(
        fragment,
        FragmentClassification::PureEl | FragmentClassification::Horn
    );
    Ok(Some(Justification {
        axioms: core,
        fragment,
        minimal_guaranteed,
    }))
}

fn fragment_of<A: ForIRI>(onto: &SetOntology<A>) -> FragmentClassification {
    owl_dl_core::convert::convert_ontology(onto)
        .map_or(FragmentClassification::OutOfFragment, |internal| {
            analyze_fragment(&internal)
        })
}

/// `QuickXplain` (Junker 2004): minimal `C' ⊆ candidates` with
/// `fixed ∪ C' ⊨ q`. Precondition: `fixed ∪ candidates ⊨ q`.
pub(crate) fn quickxplain<A: ForIRI>(
    fixed: &[Component<A>],
    candidates: &[Component<A>],
    q: &Entailment,
) -> Result<Vec<Component<A>>, ReasonError> {
    if entails(&ontology_from(fixed, &[]), q)? {
        return Ok(Vec::new()); // background alone entails ⇒ no candidate needed
    }
    if candidates.len() <= 1 {
        return Ok(candidates.to_vec());
    }
    qx(fixed, true, candidates, q)
}

/// `delta_nonempty`: whether the most recent addition to `fixed` was non-empty
/// (if empty, skip the redundant entailment check at this node).
#[allow(clippy::too_many_arguments)] // algorithm requires exactly these parameters
fn qx<A: ForIRI>(
    fixed: &[Component<A>],
    delta_nonempty: bool,
    candidates: &[Component<A>],
    q: &Entailment,
) -> Result<Vec<Component<A>>, ReasonError> {
    if delta_nonempty && entails(&ontology_from(fixed, &[]), q)? {
        return Ok(Vec::new());
    }
    if candidates.len() == 1 {
        return Ok(candidates.to_vec());
    }
    let mid = candidates.len() / 2;
    let (c1, c2) = candidates.split_at(mid);
    let fixed_c1: Vec<Component<A>> = fixed.iter().chain(c1.iter()).cloned().collect();
    let d2 = qx(&fixed_c1, !c1.is_empty(), c2, q)?;
    let fixed_d2: Vec<Component<A>> = fixed.iter().chain(d2.iter()).cloned().collect();
    let d1 = qx(&fixed_d2, !d2.is_empty(), c1, q)?;
    let mut out = d1;
    out.extend(d2);
    Ok(out)
}
