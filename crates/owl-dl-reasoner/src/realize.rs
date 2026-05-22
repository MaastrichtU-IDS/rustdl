//! Individual-level reasoning: instance checks and realization.
//!
//! All three entry points reduce to satisfiability via the standard
//! nominal trick: `KB ⊨ C(a)` iff `{a} ⊓ ¬C` is unsatisfiable in the
//! KB. The test concept seeds a fresh root node carrying both
//! `Nominal(a)` (which the tableau's nominal-assignment rule will
//! merge with the canonical witness for `a`) and `¬C`.
//!
//! Realization computes, for each declared individual, the *most
//! specific* named classes it must belong to in every model. Naive
//! implementation: for every (individual, class) pair, run an
//! instance check; then prune any class that has a strict subclass
//! also in the type set. Phase 6's saturation engine accelerates
//! the dense per-pair loop.

use std::collections::{HashMap, HashSet};

use horned_owl::model::ForIRI;
use horned_owl::ontology::set::SetOntology;
use rayon::prelude::*;

use owl_dl_core::convert::convert_ontology;
use owl_dl_core::{Axiom, ClassId, ConceptExpr, IndividualId, InternalOntology};
use owl_dl_saturation::{Subsumers, saturate};

use crate::{PreparedOntology, ReasonError, classify_internal};

/// `(entailed_types, hasse_leaves)` for one individual — returned
/// by the parallel realisation worker so the outer loop can stitch
/// the maps together.
type IndivResult = (Vec<String>, Vec<String>);

/// Decide whether `KB ⊨ class_iri(individual_iri)`. Returns `true`
/// iff `individual_iri` is provably an instance of `class_iri` in
/// every model of `ontology`.
///
/// Reduction: build the test concept `{individual_iri} ⊓ ¬class_iri`
/// and run satisfiability — instance-of holds iff *unsatisfiable*.
///
/// # Errors
///
/// See [`ReasonError`]. Unknown class or individual IRI surfaces as
/// [`ReasonError::UnknownClass`] (we reuse the same variant — a
/// dedicated `UnknownIndividual` would be a nice follow-up but
/// isn't load-bearing yet).
pub fn is_instance_of<A: ForIRI>(
    ontology: &SetOntology<A>,
    class_iri: &str,
    individual_iri: &str,
) -> Result<bool, ReasonError> {
    let internal = convert_ontology(ontology)?;
    is_instance_of_internal(&internal, class_iri, individual_iri)
}

/// Internal entry point.
///
/// # Errors
///
/// See [`ReasonError`].
pub fn is_instance_of_internal(
    internal: &InternalOntology,
    class_iri: &str,
    individual_iri: &str,
) -> Result<bool, ReasonError> {
    let class_id = internal
        .vocabulary
        .class_id(class_iri)
        .ok_or_else(|| ReasonError::UnknownClass(class_iri.to_owned()))?;
    let individual_id = internal
        .vocabulary
        .individual_id(individual_iri)
        .ok_or_else(|| ReasonError::UnknownClass(individual_iri.to_owned()))?;
    let closure = saturate(internal);
    let prepared = PreparedOntology::from_internal(internal.clone())?;
    instance_check_with_closure(internal, &closure, &prepared, class_id, individual_id)
}

/// Single instance check that consults the saturation closure first.
///
/// Three saturation fast paths, all of which short-circuit the
/// tableau. For each told class membership of `individual_id`
/// (described below), if its asserted class is a subsumer of
/// `class_id` in the EL closure, the answer is `yes`:
///
/// 1. **`ClassAssertion(D, a)`** — direct told membership.
/// 2. **`ObjectPropertyAssertion(r, a, _)` with `r`'s domain `Dom`**
///    — `a` is in `Dom` via the property domain axiom, transitively
///    via the role hierarchy.
/// 3. **`ObjectPropertyAssertion(r, _, a)` with `r`'s range `Rng`**
///    — `a` is in `Rng` via the property range axiom, transitively
///    via the role hierarchy.
///
/// Falls through to the `{a} ⊓ ¬C` satisfiability reduction otherwise.
fn instance_check_with_closure(
    internal: &InternalOntology,
    closure: &Subsumers,
    prepared: &PreparedOntology,
    class_id: ClassId,
    individual_id: IndividualId,
) -> Result<bool, ReasonError> {
    for told in told_classes_of(internal, individual_id) {
        if closure.contains(told, class_id) {
            return Ok(true);
        }
    }
    // KB ⊨ C(a) iff `{a} ⊓ ¬C` is unsatisfiable.
    let sat = prepared.decide(move |pool| {
        let cls = pool.atomic(class_id);
        let not_cls = pool.not(cls);
        let nom = pool.nominal(individual_id);
        pool.and(vec![nom, not_cls])
    })?;
    Ok(!sat)
}

/// Collect every atomic class that `individual_id` is *told* to
/// belong to:
///
/// - Direct: every `ClassAssertion(D, individual)` with `D` atomic.
/// - Via domain: every `ObjectPropertyAssertion(r, individual, _)`
///   where some `ObjectPropertyDomain(r', Dom)` axiom applies for
///   `r ⊑ r'` (named-role-only; `r'` matches when `r` and `r'`
///   share an underlying `RoleId`).
/// - Via range: every `ObjectPropertyAssertion(r, _, individual)`
///   where some `ObjectPropertyRange(r', Rng)` axiom applies under
///   the same conditions.
fn told_classes_of(internal: &InternalOntology, individual_id: IndividualId) -> Vec<ClassId> {
    let mut out = Vec::new();
    for axiom in &internal.axioms {
        match axiom {
            Axiom::ClassAssertion { class, individual } if *individual == individual_id => {
                if let ConceptExpr::Atomic(id) = internal.concepts.get(*class) {
                    out.push(*id);
                }
            }
            Axiom::ObjectPropertyAssertion {
                role,
                subject,
                object,
            } => {
                // Inverse-role property assertions: the converter
                // swaps subject/object so the stored role is always
                // named; we don't try to second-guess that here and
                // simply use `role.role_id()`.
                let used_role_id = role.role_id();
                if *subject == individual_id {
                    for dom in domains_for_role(internal, used_role_id) {
                        out.push(dom);
                    }
                }
                if *object == individual_id {
                    for rng in ranges_for_role(internal, used_role_id) {
                        out.push(rng);
                    }
                }
            }
            _ => {}
        }
    }
    out
}

fn domains_for_role(internal: &InternalOntology, role_id: owl_dl_core::RoleId) -> Vec<ClassId> {
    let mut out = Vec::new();
    for axiom in &internal.axioms {
        if let Axiom::ObjectPropertyDomain { role, domain } = axiom
            && !role.is_inverse()
            && role.role_id() == role_id
            && let ConceptExpr::Atomic(id) = internal.concepts.get(*domain)
        {
            out.push(*id);
        }
    }
    out
}

fn ranges_for_role(internal: &InternalOntology, role_id: owl_dl_core::RoleId) -> Vec<ClassId> {
    let mut out = Vec::new();
    for axiom in &internal.axioms {
        if let Axiom::ObjectPropertyRange { role, range } = axiom
            && !role.is_inverse()
            && role.role_id() == role_id
            && let ConceptExpr::Atomic(id) = internal.concepts.get(*range)
        {
            out.push(*id);
        }
    }
    out
}

/// All declared individuals that `KB` provably places in `class_iri`.
///
/// # Errors
///
/// See [`ReasonError`].
pub fn instances_of<A: ForIRI>(
    ontology: &SetOntology<A>,
    class_iri: &str,
) -> Result<Vec<String>, ReasonError> {
    let internal = convert_ontology(ontology)?;
    instances_of_internal(&internal, class_iri)
}

/// Internal entry point.
///
/// # Errors
///
/// See [`ReasonError`].
pub fn instances_of_internal(
    internal: &InternalOntology,
    class_iri: &str,
) -> Result<Vec<String>, ReasonError> {
    let class_id = internal
        .vocabulary
        .class_id(class_iri)
        .ok_or_else(|| ReasonError::UnknownClass(class_iri.to_owned()))?;
    let closure = saturate(internal);
    let prepared = PreparedOntology::from_internal(internal.clone())?;
    let mut out = Vec::new();
    for idx in 0..internal.vocabulary.num_individuals() {
        let individual_id =
            IndividualId::new(u32::try_from(idx).expect("individual count fits in u32"));
        if instance_check_with_closure(internal, &closure, &prepared, class_id, individual_id)? {
            out.push(internal.vocabulary.individual_iri(individual_id).to_owned());
        }
    }
    Ok(out)
}

/// Per-individual realization: every entailed type plus the
/// most-specific named classes (the leaves of the subclass relation
/// restricted to the entailed types).
#[derive(Debug, Clone, Default)]
pub struct Realization {
    /// All declared individual IRIs that the realization examined.
    individuals: Vec<String>,
    /// individual → all named classes entailed at that individual
    /// (full set; not the Hasse leaves).
    entailed_types: HashMap<String, Vec<String>>,
    /// individual → the most-specific entailed classes (Hasse leaves
    /// of the entailed set under the KB's subclass relation).
    most_specific_types: HashMap<String, Vec<String>>,
}

impl Realization {
    #[must_use]
    pub fn individuals(&self) -> &[String] {
        &self.individuals
    }

    #[must_use]
    pub fn entailed_types(&self, individual_iri: &str) -> &[String] {
        static EMPTY: Vec<String> = Vec::new();
        self.entailed_types
            .get(individual_iri)
            .map_or(EMPTY.as_slice(), Vec::as_slice)
    }

    #[must_use]
    pub fn most_specific_types(&self, individual_iri: &str) -> &[String] {
        static EMPTY: Vec<String> = Vec::new();
        self.most_specific_types
            .get(individual_iri)
            .map_or(EMPTY.as_slice(), Vec::as_slice)
    }
}

/// Realize every declared individual: compute entailed types and the
/// most-specific named classes per individual.
///
/// Algorithm (naive):
/// 1. Classify the ontology once to obtain the subclass matrix.
/// 2. For each individual, run an instance check against every
///    (satisfiable) class.
/// 3. From each individual's entailed-type set, prune classes that
///    have a strict subclass also in the set — leaving only the
///    Hasse leaves.
///
/// # Errors
///
/// See [`ReasonError`].
pub fn realize<A: ForIRI>(ontology: &SetOntology<A>) -> Result<Realization, ReasonError> {
    let internal = convert_ontology(ontology)?;
    realize_internal(&internal)
}

/// Internal entry point.
///
/// # Errors
///
/// See [`ReasonError`].
pub fn realize_internal(internal: &InternalOntology) -> Result<Realization, ReasonError> {
    let hierarchy = classify_internal(internal)?;
    let class_iris: Vec<String> = (0..internal.vocabulary.num_classes())
        .map(|i| {
            internal
                .vocabulary
                .class_iri(ClassId::new(
                    u32::try_from(i).expect("class count fits in u32"),
                ))
                .to_owned()
        })
        .collect();
    // Unsatisfiable classes are entailed by every individual under
    // any inconsistent slice — skip the per-individual check there;
    // we test only satisfiable classes.
    let unsat: HashSet<&str> = hierarchy.unsatisfiable_classes().into_iter().collect();
    let satisfiable: Vec<(usize, &str)> = class_iris
        .iter()
        .enumerate()
        .filter(|(_, iri)| !unsat.contains(iri.as_str()))
        .map(|(i, iri)| (i, iri.as_str()))
        .collect();

    let individual_iris: Vec<String> = (0..internal.vocabulary.num_individuals())
        .map(|i| {
            internal
                .vocabulary
                .individual_iri(IndividualId::new(
                    u32::try_from(i).expect("individual count fits in u32"),
                ))
                .to_owned()
        })
        .collect();

    let closure = saturate(internal);
    let prepared = PreparedOntology::from_internal(internal.clone())?;

    // Per-individual realization is independent across individuals
    // (each builds a fresh tableau context per class probe via
    // `prepared.decide`). Parallelise the outer loop with rayon; the
    // hierarchy / closure / prepared snapshot is shared read-only.
    let per_individual: Result<Vec<IndivResult>, ReasonError> = individual_iris
        .par_iter()
        .enumerate()
        .map(|(idx, _iri)| {
            let individual_id =
                IndividualId::new(u32::try_from(idx).expect("individual count fits in u32"));
            let mut types: Vec<&str> = Vec::new();
            for (class_idx, class_iri) in &satisfiable {
                let class_id =
                    ClassId::new(u32::try_from(*class_idx).expect("class fits in u32"));
                if instance_check_with_closure(
                    internal,
                    &closure,
                    &prepared,
                    class_id,
                    individual_id,
                )? {
                    types.push(class_iri);
                }
            }
            // Filter to Hasse leaves under the classification relation.
            let leaves: Vec<String> = types
                .iter()
                .filter(|&&cls| {
                    !types.iter().any(|&other| {
                        other != cls
                            && hierarchy.is_subclass(other, cls)
                            && !hierarchy.is_subclass(cls, other)
                    })
                })
                .map(|s| (*s).to_owned())
                .collect();
            let types_owned: Vec<String> = types.into_iter().map(str::to_owned).collect();
            Ok((types_owned, leaves))
        })
        .collect();
    let per_individual = per_individual?;
    let mut entailed_types: HashMap<String, Vec<String>> = HashMap::new();
    let mut most_specific_types: HashMap<String, Vec<String>> = HashMap::new();
    for (iri, (types_owned, leaves)) in individual_iris.iter().zip(per_individual) {
        entailed_types.insert(iri.clone(), types_owned);
        most_specific_types.insert(iri.clone(), leaves);
    }

    Ok(Realization {
        individuals: individual_iris,
        entailed_types,
        most_specific_types,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use horned_owl::io::ParserConfiguration;
    use horned_owl::io::ofn::reader::read;
    use horned_owl::model::RcStr;
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

    #[test]
    fn class_assertion_is_an_entailed_instance() {
        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(NamedIndividual(:alice))\n\
    ClassAssertion(:A :alice)\n\
)\n"
        ));
        assert!(
            is_instance_of(&onto, "http://rustdl.test/A", "http://rustdl.test/alice")
                .expect("verdict")
        );
    }

    #[test]
    fn instance_via_subsumption_chain() {
        // A ⊑ B; ClassAssertion(:A :alice) ⇒ alice : B
        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(Class(:B))\n\
    Declaration(NamedIndividual(:alice))\n\
    SubClassOf(:A :B)\n\
    ClassAssertion(:A :alice)\n\
)\n"
        ));
        assert!(
            is_instance_of(&onto, "http://rustdl.test/B", "http://rustdl.test/alice")
                .expect("verdict")
        );
    }

    #[test]
    fn non_instance_is_rejected() {
        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(Class(:B))\n\
    Declaration(NamedIndividual(:alice))\n\
    ClassAssertion(:A :alice)\n\
)\n"
        ));
        assert!(
            !is_instance_of(&onto, "http://rustdl.test/B", "http://rustdl.test/alice")
                .expect("verdict")
        );
    }

    #[test]
    fn instances_of_returns_all_known_members() {
        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(NamedIndividual(:alice))\n\
    Declaration(NamedIndividual(:bob))\n\
    Declaration(NamedIndividual(:carol))\n\
    ClassAssertion(:A :alice)\n\
    ClassAssertion(:A :bob)\n\
)\n"
        ));
        let mut members = instances_of(&onto, "http://rustdl.test/A").expect("verdict");
        members.sort();
        assert_eq!(
            members,
            vec![
                "http://rustdl.test/alice".to_owned(),
                "http://rustdl.test/bob".to_owned(),
            ]
        );
    }

    #[test]
    fn instance_check_via_property_domain() {
        // ObjectPropertyDomain(hasParent, Person);
        // ObjectPropertyAssertion(hasParent, alice, bob) ⇒
        // alice is a Person (subject of an r-edge, r's domain is
        // Person). bob is also a Person via the *range* axiom in
        // the next test.
        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:Person))\n\
    Declaration(ObjectProperty(:hasParent))\n\
    Declaration(NamedIndividual(:alice))\n\
    Declaration(NamedIndividual(:bob))\n\
    ObjectPropertyDomain(:hasParent :Person)\n\
    ObjectPropertyAssertion(:hasParent :alice :bob)\n\
)\n"
        ));
        assert!(
            is_instance_of(
                &onto,
                "http://rustdl.test/Person",
                "http://rustdl.test/alice"
            )
            .expect("verdict")
        );
    }

    #[test]
    fn instance_check_via_property_range() {
        // ObjectPropertyRange(hasParent, Person);
        // hasParent(alice, bob) ⇒ bob is a Person.
        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:Person))\n\
    Declaration(ObjectProperty(:hasParent))\n\
    Declaration(NamedIndividual(:alice))\n\
    Declaration(NamedIndividual(:bob))\n\
    ObjectPropertyRange(:hasParent :Person)\n\
    ObjectPropertyAssertion(:hasParent :alice :bob)\n\
)\n"
        ));
        assert!(
            is_instance_of(&onto, "http://rustdl.test/Person", "http://rustdl.test/bob")
                .expect("verdict")
        );
    }

    #[test]
    fn realize_filters_to_most_specific() {
        // alice : A; A ⊑ B; alice should realize as A (the leaf),
        // with B in entailed_types but not in most_specific.
        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(Class(:B))\n\
    Declaration(NamedIndividual(:alice))\n\
    SubClassOf(:A :B)\n\
    ClassAssertion(:A :alice)\n\
)\n"
        ));
        let r = realize(&onto).expect("realization");
        let alice = "http://rustdl.test/alice";
        let entailed = r.entailed_types(alice);
        assert!(entailed.iter().any(|c| c == "http://rustdl.test/A"));
        assert!(entailed.iter().any(|c| c == "http://rustdl.test/B"));
        let leaves = r.most_specific_types(alice);
        assert_eq!(leaves, vec!["http://rustdl.test/A".to_owned()]);
    }
}
