//! Black-box justification: minimal responsible-axiom sets for an entailment,
//! found by re-checking subsets of the ontology's axioms via the public
//! reasoner API. No engine internals.

use std::collections::{BTreeSet, HashSet};

use horned_owl::model::{
    Build, Class, ClassExpression, Component, DataProperty, DataPropertyAssertion,
    DifferentIndividuals, EquivalentClasses, ForIRI, Individual, Literal, MutableOntology,
    NegativeDataPropertyAssertion, NegativeObjectPropertyAssertion, ObjectProperty,
    ObjectPropertyAssertion, ObjectPropertyExpression, SameIndividual, SubObjectPropertyExpression,
};
use horned_owl::ontology::set::SetOntology;

use crate::ReasonError;
use crate::classify::{FragmentClassification, analyze_fragment};

/// An entailment to justify ("why does this hold?").
#[derive(Debug, Clone)]
pub enum Entailment {
    SubClassOf {
        sub: String,
        sup: String,
    },
    EquivalentClasses {
        a: String,
        b: String,
    },
    DisjointClasses {
        a: String,
        b: String,
    },
    Unsatisfiable {
        class: String,
    },
    InstanceOf {
        individual: String,
        class: String,
    },
    Inconsistent,
    SubObjectProperty {
        sub: String,
        sup: String,
    },
    EquivalentObjectProperties {
        a: String,
        b: String,
    },
    DisjointObjectProperties {
        a: String,
        b: String,
    },
    ObjectPropertyAssertion {
        source: String,
        prop: String,
        target: String,
    },
    SameIndividual {
        a: String,
        b: String,
    },
    DifferentIndividuals {
        a: String,
        b: String,
    },
    SubDataProperty {
        sub: String,
        sup: String,
    },
    EquivalentDataProperties {
        a: String,
        b: String,
    },
    DataPropertyValue {
        source: String,
        prop: String,
        value_lexical: String,
        value_datatype: String,
    },
}

const PROBE_IRI: &str = "urn:rustdl-justify-probe";
const PROBE_A: &str = "urn:rustdl-justify-probe-a";
const PROBE_B: &str = "urn:rustdl-justify-probe-b";
const PROBE_INT: &str = "http://www.w3.org/2001/XMLSchema#integer";

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
        Entailment::SubObjectProperty { sub, sup } => {
            let b: Build<A> = Build::new();
            inconsistent_with(
                onto,
                vec![
                    Component::ObjectPropertyAssertion(ObjectPropertyAssertion {
                        ope: ope(&b, sub),
                        from: named(&b, PROBE_A),
                        to: named(&b, PROBE_B),
                    }),
                    Component::NegativeObjectPropertyAssertion(NegativeObjectPropertyAssertion {
                        ope: ope(&b, sup),
                        from: named(&b, PROBE_A),
                        to: named(&b, PROBE_B),
                    }),
                ],
            )
        }
        Entailment::EquivalentObjectProperties { a, b } => Ok(entails(
            onto,
            &Entailment::SubObjectProperty {
                sub: a.clone(),
                sup: b.clone(),
            },
        )? && entails(
            onto,
            &Entailment::SubObjectProperty {
                sub: b.clone(),
                sup: a.clone(),
            },
        )?),
        Entailment::DisjointObjectProperties { a, b } => {
            let bld: Build<A> = Build::new();
            inconsistent_with(
                onto,
                vec![
                    Component::ObjectPropertyAssertion(ObjectPropertyAssertion {
                        ope: ope(&bld, a),
                        from: named(&bld, PROBE_A),
                        to: named(&bld, PROBE_B),
                    }),
                    Component::ObjectPropertyAssertion(ObjectPropertyAssertion {
                        ope: ope(&bld, b),
                        from: named(&bld, PROBE_A),
                        to: named(&bld, PROBE_B),
                    }),
                ],
            )
        }
        Entailment::ObjectPropertyAssertion {
            source,
            prop,
            target,
        } => {
            let b: Build<A> = Build::new();
            inconsistent_with(
                onto,
                vec![Component::NegativeObjectPropertyAssertion(
                    NegativeObjectPropertyAssertion {
                        ope: ope(&b, prop),
                        from: named(&b, source),
                        to: named(&b, target),
                    },
                )],
            )
        }
        Entailment::SameIndividual { a, b } => {
            let bld: Build<A> = Build::new();
            inconsistent_with(
                onto,
                vec![Component::DifferentIndividuals(DifferentIndividuals(vec![
                    named(&bld, a),
                    named(&bld, b),
                ]))],
            )
        }
        Entailment::DifferentIndividuals { a, b } => {
            let bld: Build<A> = Build::new();
            inconsistent_with(
                onto,
                vec![Component::SameIndividual(SameIndividual(vec![
                    named(&bld, a),
                    named(&bld, b),
                ]))],
            )
        }
        Entailment::SubDataProperty { sub, sup } => {
            let b: Build<A> = Build::new();
            let sub_lit = Literal::Datatype {
                literal: "0".to_string(),
                datatype_iri: b.iri(PROBE_INT),
            };
            let sup_lit = Literal::Datatype {
                literal: "0".to_string(),
                datatype_iri: b.iri(PROBE_INT),
            };
            // c1: asserting sub(_a,0) alone must be consistent (else range clash, not subsumption).
            if inconsistent_with(
                onto,
                vec![Component::DataPropertyAssertion(DataPropertyAssertion {
                    dp: b.data_property(sub.as_str()),
                    from: named(&b, PROBE_A),
                    to: sub_lit,
                })],
            )? {
                return Ok(false);
            }
            // c2: adding ¬sup(_a,0) becomes inconsistent ⟺ sub⊑sup forces sup(_a,0).
            inconsistent_with(
                onto,
                vec![
                    Component::DataPropertyAssertion(DataPropertyAssertion {
                        dp: b.data_property(sub.as_str()),
                        from: named(&b, PROBE_A),
                        to: Literal::Datatype {
                            literal: "0".to_string(),
                            datatype_iri: b.iri(PROBE_INT),
                        },
                    }),
                    Component::NegativeDataPropertyAssertion(NegativeDataPropertyAssertion {
                        dp: b.data_property(sup.as_str()),
                        from: named(&b, PROBE_A),
                        to: sup_lit,
                    }),
                ],
            )
        }
        Entailment::EquivalentDataProperties { a, b } => Ok(entails(
            onto,
            &Entailment::SubDataProperty {
                sub: a.clone(),
                sup: b.clone(),
            },
        )? && entails(
            onto,
            &Entailment::SubDataProperty {
                sub: b.clone(),
                sup: a.clone(),
            },
        )?),
        Entailment::DataPropertyValue {
            source,
            prop,
            value_lexical,
            value_datatype,
        } => {
            let b: Build<A> = Build::new();
            let lit = Literal::Datatype {
                literal: value_lexical.clone(),
                datatype_iri: b.iri(value_datatype.as_str()),
            };
            inconsistent_with(
                onto,
                vec![Component::NegativeDataPropertyAssertion(
                    NegativeDataPropertyAssertion {
                        dp: b.data_property(prop.as_str()),
                        from: named(&b, source),
                        to: lit,
                    },
                )],
            )
        }
    }
}

/// `true` iff `onto ∪ extra` is inconsistent. The `extra` axioms are query-
/// encoding probes (fresh `PROBE_*` symbols), never candidate axioms — they
/// appear in every tested subset and never in a justification.
fn inconsistent_with<A: ForIRI>(
    onto: &SetOntology<A>,
    extra: Vec<Component<A>>,
) -> Result<bool, ReasonError> {
    let mut probed = onto.clone();
    for c in extra {
        probed.insert(c);
    }
    Ok(!crate::is_consistent(&probed)?)
}

fn named<A: ForIRI>(b: &Build<A>, iri: &str) -> Individual<A> {
    Individual::Named(b.named_individual(iri))
}

fn ope<A: ForIRI>(b: &Build<A>, iri: &str) -> ObjectPropertyExpression<A> {
    ObjectPropertyExpression::ObjectProperty(b.object_property(iri))
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

const OWL_THING: &str = "http://www.w3.org/2002/07/owl#Thing";
const OWL_NOTHING: &str = "http://www.w3.org/2002/07/owl#Nothing";

fn op_iri<A: ForIRI>(p: &ObjectProperty<A>) -> &str {
    p.0.as_ref()
}
fn dp_iri<A: ForIRI>(p: &DataProperty<A>) -> &str {
    p.0.as_ref()
}
fn cls_iri<A: ForIRI>(c: &Class<A>) -> &str {
    c.0.as_ref()
}

/// The underlying property IRI of an object-property expression. `Inv(R)` is
/// keyed by `R` — for ⊥-locality, `Inv(R)` is the empty role iff `R` is.
fn ope_iri<A: ForIRI>(ope: &ObjectPropertyExpression<A>) -> &str {
    match ope {
        ObjectPropertyExpression::ObjectProperty(p)
        | ObjectPropertyExpression::InverseObjectProperty(p) => p.0.as_ref(),
    }
}

fn ind_iri<A: ForIRI>(i: &Individual<A>) -> Option<&str> {
    match i {
        Individual::Named(n) => Some(n.0.as_ref()),
        Individual::Anonymous(_) => None,
    }
}

/// Is `ce` provably ⊥ ("equivalent to owl:Nothing") under ⊥-locality w.r.t.
/// `sig`? External class names (∉ `sig`) and external roles map to ⊥/empty.
/// Returns `true` only when provably ⊥; unknown constructs return `false`.
fn ce_is_bot<A: ForIRI>(ce: &ClassExpression<A>, sig: &HashSet<String>) -> bool {
    use ClassExpression as CE;
    match ce {
        CE::Class(c) => {
            let iri = c.0.as_ref();
            iri == OWL_NOTHING || (iri != OWL_THING && !sig.contains(iri))
        }
        CE::ObjectComplementOf(c) => ce_is_top(c, sig),
        CE::ObjectIntersectionOf(cs) => cs.iter().any(|c| ce_is_bot(c, sig)),
        CE::ObjectUnionOf(cs) => !cs.is_empty() && cs.iter().all(|c| ce_is_bot(c, sig)),
        CE::ObjectSomeValuesFrom { ope, bce } => !sig.contains(ope_iri(ope)) || ce_is_bot(bce, sig),
        CE::ObjectHasValue { ope, .. } | CE::ObjectHasSelf(ope) => !sig.contains(ope_iri(ope)),
        CE::ObjectMinCardinality { n, ope, bce } | CE::ObjectExactCardinality { n, ope, bce } => {
            *n >= 1 && (!sig.contains(ope_iri(ope)) || ce_is_bot(bce, sig))
        }
        CE::ObjectOneOf(inds) => inds.is_empty(),
        CE::DataSomeValuesFrom { dp, .. } | CE::DataHasValue { dp, .. } => {
            !sig.contains(dp.0.as_ref())
        }
        CE::DataMinCardinality { n, dp, .. } | CE::DataExactCardinality { n, dp, .. } => {
            *n >= 1 && !sig.contains(dp.0.as_ref())
        }
        _ => false,
    }
}

/// Is `ce` provably ⊤ ("equivalent to owl:Thing") under ⊥-locality w.r.t.
/// `sig`? Returns `true` only when provably ⊤; unknown constructs return
/// `false`.
fn ce_is_top<A: ForIRI>(ce: &ClassExpression<A>, sig: &HashSet<String>) -> bool {
    use ClassExpression as CE;
    match ce {
        CE::Class(c) => c.0.as_ref() == OWL_THING,
        CE::ObjectComplementOf(c) => ce_is_bot(c, sig),
        CE::ObjectIntersectionOf(cs) => cs.iter().all(|c| ce_is_top(c, sig)),
        CE::ObjectUnionOf(cs) => cs.iter().any(|c| ce_is_top(c, sig)),
        CE::ObjectAllValuesFrom { ope, bce } => !sig.contains(ope_iri(ope)) || ce_is_top(bce, sig),
        CE::ObjectMaxCardinality { ope, bce, .. } => {
            !sig.contains(ope_iri(ope)) || ce_is_bot(bce, sig)
        }
        CE::ObjectMinCardinality { n, .. } | CE::DataMinCardinality { n, .. } => *n == 0,
        CE::ObjectExactCardinality { n, ope, bce } => {
            *n == 0 && (!sig.contains(ope_iri(ope)) || ce_is_bot(bce, sig))
        }
        CE::DataAllValuesFrom { dp, .. } | CE::DataMaxCardinality { dp, .. } => {
            !sig.contains(dp.0.as_ref())
        }
        _ => false,
    }
}

/// Is the sub-side of a property inclusion provably the empty role under
/// ⊥-locality? A chain is empty iff any component is external.
fn sub_ope_is_bot<A: ForIRI>(sub: &SubObjectPropertyExpression<A>, sig: &HashSet<String>) -> bool {
    match sub {
        SubObjectPropertyExpression::ObjectPropertyExpression(ope) => !sig.contains(ope_iri(ope)),
        SubObjectPropertyExpression::ObjectPropertyChain(chain) => {
            !chain.is_empty() && chain.iter().any(|ope| !sig.contains(ope_iri(ope)))
        }
    }
}

/// Is `c` ⊥-local w.r.t. `sig`? A ⊥-local axiom becomes a tautology when every
/// term outside `sig` is replaced by ⊥/empty, so it cannot belong to any
/// justification over `sig`. **Conservative: returns `true` only when provably
/// local; every unhandled construct returns `false` (non-local ⇒ kept).** This
/// under-approximates locality, so [`extract_bot_module`] yields a *superset*
/// of the true ⊥-module and never drops a justification axiom.
// Arms share bodies but cannot be merged: each `Component` variant wraps a
// distinct newtype, so an or-pattern binding `a` would fail to typecheck.
#[allow(clippy::match_same_arms)]
fn is_bot_local<A: ForIRI>(c: &Component<A>, sig: &HashSet<String>) -> bool {
    use Component as C;
    match c {
        C::SubClassOf(a) => ce_is_bot(&a.sub, sig) || ce_is_top(&a.sup, sig),
        C::EquivalentClasses(a) => {
            a.0.iter().all(|c| ce_is_bot(c, sig)) || a.0.iter().all(|c| ce_is_top(c, sig))
        }
        // Disjoint holds vacuously once at most one disjunct is non-empty.
        C::DisjointClasses(a) => a.0.iter().filter(|c| !ce_is_bot(c, sig)).count() <= 1,
        C::SubObjectPropertyOf(a) => sub_ope_is_bot(&a.sub, sig),
        C::ObjectPropertyDomain(a) => !sig.contains(ope_iri(&a.ope)) || ce_is_top(&a.ce, sig),
        C::ObjectPropertyRange(a) => !sig.contains(ope_iri(&a.ope)) || ce_is_top(&a.ce, sig),
        C::DataPropertyDomain(a) => !sig.contains(dp_iri(&a.dp)) || ce_is_top(&a.ce, sig),
        C::FunctionalObjectProperty(a) => !sig.contains(ope_iri(&a.0)),
        C::InverseFunctionalObjectProperty(a) => !sig.contains(ope_iri(&a.0)),
        C::TransitiveObjectProperty(a) => !sig.contains(ope_iri(&a.0)),
        C::SymmetricObjectProperty(a) => !sig.contains(ope_iri(&a.0)),
        C::AsymmetricObjectProperty(a) => !sig.contains(ope_iri(&a.0)),
        C::IrreflexiveObjectProperty(a) => !sig.contains(ope_iri(&a.0)),
        C::FunctionalDataProperty(a) => !sig.contains(dp_iri(&a.0)),
        C::DataPropertyRange(a) => !sig.contains(dp_iri(&a.dp)),
        C::SubDataPropertyOf(a) => !sig.contains(dp_iri(&a.sub)),
        C::InverseObjectProperties(a) => !sig.contains(op_iri(&a.0)) && !sig.contains(op_iri(&a.1)),
        C::EquivalentObjectProperties(a) => a.0.iter().all(|p| !sig.contains(ope_iri(p))),
        C::DisjointObjectProperties(a) => {
            a.0.iter().filter(|p| sig.contains(ope_iri(p))).count() <= 1
        }
        C::EquivalentDataProperties(a) => a.0.iter().all(|p| !sig.contains(dp_iri(p))),
        C::DisjointDataProperties(a) => a.0.iter().filter(|p| sig.contains(dp_iri(p))).count() <= 1,
        // ReflexiveObjectProperty (⊤ ⊑ ∃R.Self), ABox assertions, HasKey,
        // DisjointUnion, datatype defs, and anything else: NOT provably local.
        _ => false,
    }
}

/// Add every named-entity IRI occurring in `ce` to `out`.
fn collect_ce_entities<A: ForIRI>(ce: &ClassExpression<A>, out: &mut HashSet<String>) {
    use ClassExpression as CE;
    match ce {
        CE::Class(c) => {
            out.insert(c.0.as_ref().to_string());
        }
        CE::ObjectComplementOf(c) => collect_ce_entities(c, out),
        CE::ObjectIntersectionOf(cs) | CE::ObjectUnionOf(cs) => {
            for c in cs {
                collect_ce_entities(c, out);
            }
        }
        CE::ObjectSomeValuesFrom { ope, bce }
        | CE::ObjectAllValuesFrom { ope, bce }
        | CE::ObjectMinCardinality { ope, bce, .. }
        | CE::ObjectMaxCardinality { ope, bce, .. }
        | CE::ObjectExactCardinality { ope, bce, .. } => {
            out.insert(ope_iri(ope).to_string());
            collect_ce_entities(bce, out);
        }
        CE::ObjectHasValue { ope, i } => {
            out.insert(ope_iri(ope).to_string());
            if let Some(s) = ind_iri(i) {
                out.insert(s.to_string());
            }
        }
        CE::ObjectHasSelf(ope) => {
            out.insert(ope_iri(ope).to_string());
        }
        CE::ObjectOneOf(inds) => {
            for i in inds {
                if let Some(s) = ind_iri(i) {
                    out.insert(s.to_string());
                }
            }
        }
        CE::DataSomeValuesFrom { dp, .. }
        | CE::DataAllValuesFrom { dp, .. }
        | CE::DataHasValue { dp, .. }
        | CE::DataMinCardinality { dp, .. }
        | CE::DataMaxCardinality { dp, .. }
        | CE::DataExactCardinality { dp, .. } => {
            out.insert(dp.0.as_ref().to_string());
        }
    }
}

/// Add every named-entity IRI occurring in `c` to `out`. Covers all logical
/// axiom shapes; unhandled shapes contribute nothing (safe — the axiom is still
/// kept, only the signature does not grow from it).
// Arms share bodies but cannot be merged (distinct per-variant newtypes).
#[allow(clippy::match_same_arms)]
fn collect_component_entities<A: ForIRI>(c: &Component<A>, out: &mut HashSet<String>) {
    use Component as C;
    macro_rules! add {
        ($iri:expr) => {{
            out.insert(($iri).to_string());
        }};
    }
    match c {
        C::SubClassOf(a) => {
            collect_ce_entities(&a.sub, out);
            collect_ce_entities(&a.sup, out);
        }
        C::EquivalentClasses(a) => {
            for x in &a.0 {
                collect_ce_entities(x, out);
            }
        }
        C::DisjointClasses(a) => {
            for x in &a.0 {
                collect_ce_entities(x, out);
            }
        }
        C::DisjointUnion(a) => {
            add!(cls_iri(&a.0));
            for x in &a.1 {
                collect_ce_entities(x, out);
            }
        }
        C::SubObjectPropertyOf(a) => {
            add!(ope_iri(&a.sup));
            match &a.sub {
                SubObjectPropertyExpression::ObjectPropertyExpression(ope) => add!(ope_iri(ope)),
                SubObjectPropertyExpression::ObjectPropertyChain(chain) => {
                    for ope in chain {
                        add!(ope_iri(ope));
                    }
                }
            }
        }
        C::EquivalentObjectProperties(a) => {
            for p in &a.0 {
                add!(ope_iri(p));
            }
        }
        C::DisjointObjectProperties(a) => {
            for p in &a.0 {
                add!(ope_iri(p));
            }
        }
        C::InverseObjectProperties(a) => {
            add!(op_iri(&a.0));
            add!(op_iri(&a.1));
        }
        C::ObjectPropertyDomain(a) => {
            add!(ope_iri(&a.ope));
            collect_ce_entities(&a.ce, out);
        }
        C::ObjectPropertyRange(a) => {
            add!(ope_iri(&a.ope));
            collect_ce_entities(&a.ce, out);
        }
        C::FunctionalObjectProperty(a) => add!(ope_iri(&a.0)),
        C::InverseFunctionalObjectProperty(a) => add!(ope_iri(&a.0)),
        C::TransitiveObjectProperty(a) => add!(ope_iri(&a.0)),
        C::SymmetricObjectProperty(a) => add!(ope_iri(&a.0)),
        C::AsymmetricObjectProperty(a) => add!(ope_iri(&a.0)),
        C::ReflexiveObjectProperty(a) => add!(ope_iri(&a.0)),
        C::IrreflexiveObjectProperty(a) => add!(ope_iri(&a.0)),
        C::SubDataPropertyOf(a) => {
            add!(dp_iri(&a.sup));
            add!(dp_iri(&a.sub));
        }
        C::EquivalentDataProperties(a) => {
            for p in &a.0 {
                add!(dp_iri(p));
            }
        }
        C::DisjointDataProperties(a) => {
            for p in &a.0 {
                add!(dp_iri(p));
            }
        }
        C::DataPropertyDomain(a) => {
            add!(dp_iri(&a.dp));
            collect_ce_entities(&a.ce, out);
        }
        C::DataPropertyRange(a) => add!(dp_iri(&a.dp)),
        C::FunctionalDataProperty(a) => add!(dp_iri(&a.0)),
        C::HasKey(a) => collect_ce_entities(&a.ce, out),
        C::ClassAssertion(a) => {
            collect_ce_entities(&a.ce, out);
            if let Some(s) = ind_iri(&a.i) {
                add!(s);
            }
        }
        C::ObjectPropertyAssertion(a) => {
            add!(ope_iri(&a.ope));
            for i in [&a.from, &a.to] {
                if let Some(s) = ind_iri(i) {
                    add!(s);
                }
            }
        }
        C::NegativeObjectPropertyAssertion(a) => {
            add!(ope_iri(&a.ope));
            for i in [&a.from, &a.to] {
                if let Some(s) = ind_iri(i) {
                    add!(s);
                }
            }
        }
        C::DataPropertyAssertion(a) => {
            add!(dp_iri(&a.dp));
            if let Some(s) = ind_iri(&a.from) {
                add!(s);
            }
        }
        C::NegativeDataPropertyAssertion(a) => {
            add!(dp_iri(&a.dp));
            if let Some(s) = ind_iri(&a.from) {
                add!(s);
            }
        }
        C::SameIndividual(a) => {
            for i in &a.0 {
                if let Some(s) = ind_iri(i) {
                    add!(s);
                }
            }
        }
        C::DifferentIndividuals(a) => {
            for i in &a.0 {
                if let Some(s) = ind_iri(i) {
                    add!(s);
                }
            }
        }
        _ => {}
    }
}

/// All named-entity IRIs (classes, object/data properties, named individuals)
/// occurring in `c`. Exposed for callers that gloss an axiom by entity — e.g.
/// the CLI's `--labels` rendering. Deterministic order.
#[must_use]
pub fn component_entities<A: ForIRI>(c: &Component<A>) -> BTreeSet<String> {
    let mut s = HashSet::new();
    collect_component_entities(c, &mut s);
    s.into_iter().collect()
}

/// The seed signature of a query: the entity IRIs the justification is "about".
/// `None` ⇒ the query is not localizable (e.g. global inconsistency); the
/// caller keeps the full axiom set.
fn query_seed_signature(q: &Entailment) -> Option<HashSet<String>> {
    let mut s = HashSet::new();
    match q {
        Entailment::SubClassOf { sub, sup }
        | Entailment::SubObjectProperty { sub, sup }
        | Entailment::SubDataProperty { sub, sup } => {
            s.insert(sub.clone());
            s.insert(sup.clone());
        }
        Entailment::EquivalentClasses { a, b }
        | Entailment::DisjointClasses { a, b }
        | Entailment::EquivalentObjectProperties { a, b }
        | Entailment::DisjointObjectProperties { a, b }
        | Entailment::SameIndividual { a, b }
        | Entailment::DifferentIndividuals { a, b }
        | Entailment::EquivalentDataProperties { a, b } => {
            s.insert(a.clone());
            s.insert(b.clone());
        }
        Entailment::Unsatisfiable { class } => {
            s.insert(class.clone());
        }
        Entailment::InstanceOf { individual, class } => {
            s.insert(individual.clone());
            s.insert(class.clone());
        }
        Entailment::ObjectPropertyAssertion {
            source,
            prop,
            target,
        } => {
            s.insert(source.clone());
            s.insert(prop.clone());
            s.insert(target.clone());
        }
        Entailment::DataPropertyValue { source, prop, .. } => {
            s.insert(source.clone());
            s.insert(prop.clone());
        }
        Entailment::Inconsistent => return None,
    }
    Some(s)
}

/// Extract the syntactic ⊥-locality module of `candidates` for seed signature
/// `seed`: the fixpoint of "keep every axiom that is not ⊥-local w.r.t. the
/// signature accumulated so far, then grow the signature by that axiom's
/// terms." Justification-preserving — every justification of an entailment over
/// `seed` lies within the returned module.
#[must_use]
#[allow(clippy::implicit_hasher)] // seed is always a default-hasher HashSet here
pub fn extract_bot_module<A: ForIRI>(
    candidates: &[Component<A>],
    seed: &HashSet<String>,
) -> Vec<Component<A>> {
    let mut sig = seed.clone();
    let mut in_module = vec![false; candidates.len()];
    let mut changed = true;
    while changed {
        changed = false;
        for (i, ax) in candidates.iter().enumerate() {
            if in_module[i] || is_bot_local(ax, &sig) {
                continue;
            }
            in_module[i] = true;
            collect_component_entities(ax, &mut sig);
            changed = true;
        }
    }
    candidates
        .iter()
        .zip(&in_module)
        .filter(|(_, keep)| **keep)
        .map(|(ax, _)| ax.clone())
        .collect()
}

/// Decide entailment and pick the candidate set to search, reasoning over the
/// query's ⊥-module instead of the whole ontology whenever possible.
///
/// Returns `Ok(None)` when `q` is not entailed, else `Ok(Some(candidates))`.
/// The ⊥-module is extracted *syntactically* (no reasoning) and is both
/// justification- and entailment-preserving for the seed signature, so the
/// entailment check runs on the small module — never on full wine/sio. If the
/// module unexpectedly fails to entail (a locality-classifier bug dropping a
/// needed axiom), we re-check the full set and fall back to it: this keeps
/// find-one sound and find-all complete even if `is_bot_local` is wrong, paying
/// the full-ontology cost only on that (rare) negative.
fn localized_candidates<A: ForIRI>(
    fixed: &[Component<A>],
    all_candidates: &[Component<A>],
    q: &Entailment,
) -> Result<Option<Vec<Component<A>>>, ReasonError> {
    // Escape hatch / differential-test gate: skip module extraction entirely.
    let no_module = std::env::var_os("RUSTDL_JUSTIFY_NO_MODULE").is_some();
    let Some(seed) = (if no_module {
        None
    } else {
        query_seed_signature(q)
    }) else {
        // Not localizable (or disabled): decide on the full set.
        return Ok(
            entails(&ontology_from(fixed, all_candidates), q)?.then(|| all_candidates.to_vec())
        );
    };
    let module = extract_bot_module(all_candidates, &seed);
    if std::env::var_os("RUSTDL_JUSTIFY_DEBUG").is_some() {
        eprintln!(
            "# justify ⊥-module: {} of {} logical axioms",
            module.len(),
            all_candidates.len()
        );
    }
    if entails(&ontology_from(fixed, &module), q)? {
        Ok(Some(module))
    } else if entails(&ontology_from(fixed, all_candidates), q)? {
        Ok(Some(all_candidates.to_vec())) // locality bug — safe fallback
    } else {
        Ok(None) // genuinely not entailed
    }
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
    let (fixed, all_candidates) = logical_axioms(onto);
    let Some(candidates) = localized_candidates(&fixed, &all_candidates, q)? else {
        return Ok(None); // not entailed — nothing to justify
    };
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

/// Find up to `max` minimal justifications for `q` via a Reiter Hitting-Set
/// Tree over [`quickxplain`] (`QuickXplain`). Returns `[]` if `q` is not entailed.
///
/// When capped, the returned justifications are the first `max` found in the
/// HST's DFS traversal order — no particular preference (e.g. smallest) among
/// them is guaranteed.
///
/// # Errors
/// Propagates [`ReasonError`].
pub fn find_all_justifications<A: ForIRI>(
    onto: &SetOntology<A>,
    q: &Entailment,
    max: usize,
) -> Result<Vec<Justification<A>>, ReasonError> {
    let (fixed, all_candidates) = logical_axioms(onto);
    // Narrow to the query's ⊥-module up front: justification-preserving, so the
    // HST below still discovers *every* justification — over a far smaller set.
    let Some(candidates) = localized_candidates(&fixed, &all_candidates, q)? else {
        return Ok(Vec::new()); // not entailed
    };
    let mut found: Vec<Vec<Component<A>>> = Vec::new();
    let mut seen: std::collections::HashSet<BTreeSet<Component<A>>> =
        std::collections::HashSet::new();
    // HST worklist: each node is a set of candidate-INDICES removed on the path.
    let mut worklist: Vec<BTreeSet<usize>> = vec![BTreeSet::new()];
    let mut explored: BTreeSet<BTreeSet<usize>> = BTreeSet::new();
    while let Some(removed) = worklist.pop() {
        if found.len() >= max {
            break;
        }
        if !explored.insert(removed.clone()) {
            continue;
        }
        let subset: Vec<Component<A>> = candidates
            .iter()
            .enumerate()
            .filter(|(i, _)| !removed.contains(i))
            .map(|(_, c)| c.clone())
            .collect();
        if !entails(&ontology_from(&fixed, &subset), q)? {
            continue; // this branch cannot yield a justification
        }
        let j = quickxplain(&fixed, &subset, q)?;
        // Record if this justification (as an axiom SET) is new.
        let key: BTreeSet<Component<A>> = j.iter().cloned().collect();
        if seen.insert(key) {
            found.push(j.clone());
        }
        // Branch: remove each justification axiom (by its candidate index).
        for c in &j {
            if let Some(idx) = candidates.iter().position(|x| x == c) {
                let mut next = removed.clone();
                next.insert(idx);
                worklist.push(next);
            }
        }
    }
    let fragment = fragment_of(onto);
    let minimal_guaranteed = matches!(
        fragment,
        FragmentClassification::PureEl | FragmentClassification::Horn
    );
    Ok(found
        .into_iter()
        .map(|axioms| Justification {
            axioms,
            fragment,
            minimal_guaranteed,
        })
        .collect())
}

/// `delta_nonempty`: whether the most recent addition to `fixed` was non-empty
/// (if empty, skip the redundant entailment check at this node).
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
