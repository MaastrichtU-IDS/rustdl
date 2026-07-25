//! Complex (anonymous) class-expression queries (issue #48). A parsed
//! `ClassExpression` is answered by minting a fresh probe class `Q`, adding
//! `EquivalentClasses(Q, CE)`, and delegating to the named-class queries — the
//! same reduction `justify::entails` uses for its `DisjointClasses` arm. Sound by
//! construction (a definitional extension over a fresh name adds no entailment
//! about the original signature). Read-only on the input.

use std::collections::HashSet;

use crate::{
    QueryStats, ReasonError, instances_of, is_class_satisfiable_with_stats,
    is_subclass_of_with_stats,
};
use horned_owl::model::{
    Build, ClassExpression, Component, EquivalentClasses, ForIRI, MutableOntology,
};
use horned_owl::ontology::set::SetOntology;

const PROBE_IRI: &str = "urn:rustdl-ce-probe:q";
const PROBE_IRI_2: &str = "urn:rustdl-ce-probe:q2";

/// Verdict for a complex-class-expression satisfiability/subsumption query.
#[derive(Debug, Clone, Copy)]
pub struct CeVerdict {
    holds: bool,
    incomplete: bool,
}

impl CeVerdict {
    /// Whether the query holds.
    #[must_use]
    pub fn holds(&self) -> bool {
        self.holds
    }

    /// Whether the underlying probe query took a path that is not
    /// guaranteed complete (i.e. answered off the pure-EL fast path).
    #[must_use]
    pub fn incomplete(&self) -> bool {
        self.incomplete
    }
}

/// Result of a complex-class-expression instance query.
#[derive(Debug, Clone)]
pub struct CeInstances {
    individuals: Vec<String>,
    incomplete: bool,
}

impl CeInstances {
    /// The individuals provably in the queried class expression.
    #[must_use]
    pub fn individuals(&self) -> &[String] {
        &self.individuals
    }

    /// Whether the underlying probe query took a path that is not
    /// guaranteed complete.
    #[must_use]
    pub fn incomplete(&self) -> bool {
        self.incomplete
    }
}

fn incomplete_of(stats: QueryStats) -> bool {
    !stats.pure_el_mode
}

/// Add every class IRI referenced (nested) inside `ce` to `out`. Mirrors
/// `justify::collect_ce_entities`'s recursion shape but only ever inserts on
/// the `ClassExpression::Class` arm — properties/individuals/data-property
/// fillers are irrelevant to "does this IRI denote a class".
fn collect_ce_classes<A: ForIRI>(ce: &ClassExpression<A>, out: &mut HashSet<String>) {
    use ClassExpression as CE;
    match ce {
        CE::Class(c) => {
            out.insert(c.0.as_ref().to_string());
        }
        CE::ObjectComplementOf(c) => collect_ce_classes(c, out),
        CE::ObjectIntersectionOf(cs) | CE::ObjectUnionOf(cs) => {
            for c in cs {
                collect_ce_classes(c, out);
            }
        }
        CE::ObjectSomeValuesFrom { bce, .. }
        | CE::ObjectAllValuesFrom { bce, .. }
        | CE::ObjectMinCardinality { bce, .. }
        | CE::ObjectMaxCardinality { bce, .. }
        | CE::ObjectExactCardinality { bce, .. } => collect_ce_classes(bce, out),
        // ObjectHasValue/ObjectHasSelf/ObjectOneOf (individuals, not classes) and
        // the Data* variants (datatype fillers) contribute no class IRI.
        _ => {}
    }
}

/// The full class signature of `onto`: every IRI that denotes a class
/// anywhere — `Declaration(Class(...))`, a `DisjointUnion` definiendum, or a
/// `ClassExpression::Class` occurring (recursively) in any axiom. Broader
/// than scanning declarations alone: an IRI can be *used* as a class in e.g.
/// `SubClassOf(<iri> Real)` with no accompanying `Declaration(Class(...))`,
/// and the probe-freshness invariant must catch that too (horned-owl 1.4 /
/// this pinned fork exposes no ready-made "ontology class signature" API —
/// no `signature()`/`Signatured` trait — so this walks `Component` by hand).
fn class_signature<A: ForIRI>(onto: &SetOntology<A>) -> HashSet<String> {
    let mut out = HashSet::new();
    for ac in onto {
        match &ac.component {
            Component::DeclareClass(dc) => {
                out.insert(dc.0.0.as_ref().to_string());
            }
            Component::SubClassOf(a) => {
                collect_ce_classes(&a.sub, &mut out);
                collect_ce_classes(&a.sup, &mut out);
            }
            Component::EquivalentClasses(a) => {
                for x in &a.0 {
                    collect_ce_classes(x, &mut out);
                }
            }
            Component::DisjointClasses(a) => {
                for x in &a.0 {
                    collect_ce_classes(x, &mut out);
                }
            }
            Component::DisjointUnion(a) => {
                out.insert(a.0.0.as_ref().to_string());
                for x in &a.1 {
                    collect_ce_classes(x, &mut out);
                }
            }
            Component::ObjectPropertyDomain(a) => collect_ce_classes(&a.ce, &mut out),
            Component::ObjectPropertyRange(a) => collect_ce_classes(&a.ce, &mut out),
            Component::DataPropertyDomain(a) => collect_ce_classes(&a.ce, &mut out),
            Component::HasKey(a) => collect_ce_classes(&a.ce, &mut out),
            Component::ClassAssertion(a) => collect_ce_classes(&a.ce, &mut out),
            _ => {}
        }
    }
    out
}

/// Error if `iri` already occurs as a class anywhere in the ontology's class
/// signature — the probe must be fresh, never silently overwriting or
/// colliding with a pre-existing class (declared, or merely referenced).
fn ensure_fresh<A: ForIRI>(onto: &SetOntology<A>, iri: &str) -> Result<(), ReasonError> {
    if class_signature(onto).contains(iri) {
        return Err(ReasonError::UnknownClass(format!(
            "probe IRI {iri} collides with a declared class"
        )));
    }
    Ok(())
}

fn probe_axiom<A: ForIRI>(build: &Build<A>, iri: &str, ce: &ClassExpression<A>) -> Component<A> {
    Component::EquivalentClasses(EquivalentClasses(vec![
        ClassExpression::Class(build.class(iri)),
        ce.clone(),
    ]))
}

/// Is `ce` satisfiable w.r.t. `onto`? Reduces to satisfiability of a fresh
/// probe class defined as `Q ≡ ce`.
///
/// # Errors
/// [`ReasonError::UnknownClass`] if the probe IRI collides with an existing
/// declared class; otherwise propagates reasoner errors.
pub fn class_expression_satisfiable<A: ForIRI>(
    onto: &SetOntology<A>,
    ce: &ClassExpression<A>,
) -> Result<CeVerdict, ReasonError> {
    ensure_fresh(onto, PROBE_IRI)?;
    let mut probed = onto.clone();
    let build: Build<A> = Build::new();
    probed.insert(probe_axiom(&build, PROBE_IRI, ce));
    let (holds, stats) = is_class_satisfiable_with_stats(&probed, PROBE_IRI)?;
    Ok(CeVerdict {
        holds,
        incomplete: incomplete_of(stats),
    })
}

/// Is `sub_ce ⊑ sup_ce` entailed by `onto`? Reduces to subsumption between two
/// fresh probe classes each defined as equivalent to one of the operands.
///
/// # Errors
/// As [`class_expression_satisfiable`].
pub fn class_expression_entailed_subclass<A: ForIRI>(
    onto: &SetOntology<A>,
    sub_ce: &ClassExpression<A>,
    sup_ce: &ClassExpression<A>,
) -> Result<CeVerdict, ReasonError> {
    ensure_fresh(onto, PROBE_IRI)?;
    ensure_fresh(onto, PROBE_IRI_2)?;
    let mut probed = onto.clone();
    let build: Build<A> = Build::new();
    probed.insert(probe_axiom(&build, PROBE_IRI, sub_ce));
    probed.insert(probe_axiom(&build, PROBE_IRI_2, sup_ce));
    let (holds, stats) = is_subclass_of_with_stats(&probed, PROBE_IRI, PROBE_IRI_2)?;
    Ok(CeVerdict {
        holds,
        incomplete: incomplete_of(stats),
    })
}

/// The individuals provably in `ce` w.r.t. `onto`. Reduces to the named-class
/// instance query over a fresh probe class defined as `Q ≡ ce`; the synthetic
/// probe IRI itself is filtered out of the result.
///
/// # Errors
/// As [`class_expression_satisfiable`].
pub fn class_expression_instances<A: ForIRI>(
    onto: &SetOntology<A>,
    ce: &ClassExpression<A>,
) -> Result<CeInstances, ReasonError> {
    ensure_fresh(onto, PROBE_IRI)?;
    let mut probed = onto.clone();
    let build: Build<A> = Build::new();
    probed.insert(probe_axiom(&build, PROBE_IRI, ce));
    // Completeness signal via a companion sat query on the probe (cheap vs realize).
    let (_sat, stats) = is_class_satisfiable_with_stats(&probed, PROBE_IRI)?;
    let mut individuals = instances_of(&probed, PROBE_IRI)?;
    individuals.retain(|i| !i.starts_with("urn:rustdl-ce-probe"));
    individuals.sort();
    individuals.dedup();
    Ok(CeInstances {
        individuals,
        incomplete: incomplete_of(stats),
    })
}
