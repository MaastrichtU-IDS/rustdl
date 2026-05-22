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

use owl_dl_core::convert::convert_ontology;
use owl_dl_core::{Axiom, ClassId, ConceptExpr, IndividualId, InternalOntology};
use owl_dl_saturation::{Subsumers, saturate};

use crate::{PreparedOntology, ReasonError, classify_internal};

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

/// Single instance check that consults the saturation closure first:
/// if any told `ClassAssertion(D, individual)` has `D ⊑ class_id` in
/// the EL closure, the answer is `yes` without invoking the tableau.
/// Otherwise falls through to the standard `{a} ⊓ ¬C` satisfiability
/// reduction against the prepared ontology.
fn instance_check_with_closure(
    internal: &InternalOntology,
    closure: &Subsumers,
    prepared: &PreparedOntology,
    class_id: ClassId,
    individual_id: IndividualId,
) -> Result<bool, ReasonError> {
    // Saturation fast path: walk the told class assertions for this
    // individual; if any of them is a saturation-subsumer of the
    // target class, we're done.
    for axiom in &internal.axioms {
        if let Axiom::ClassAssertion { class, individual } = axiom
            && *individual == individual_id
            && let ConceptExpr::Atomic(asserted) = internal.concepts.get(*class)
            && closure.contains(*asserted, class_id)
        {
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

    let mut entailed_types: HashMap<String, Vec<String>> = HashMap::new();
    let mut most_specific_types: HashMap<String, Vec<String>> = HashMap::new();
    let closure = saturate(internal);
    let prepared = PreparedOntology::from_internal(internal.clone())?;

    for (idx, iri) in individual_iris.iter().enumerate() {
        let individual_id =
            IndividualId::new(u32::try_from(idx).expect("individual count fits in u32"));
        let mut types: Vec<&str> = Vec::new();
        for (class_idx, class_iri) in &satisfiable {
            let class_id = ClassId::new(u32::try_from(*class_idx).expect("class fits in u32"));
            if instance_check_with_closure(internal, &closure, &prepared, class_id, individual_id)?
            {
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
        entailed_types.insert(iri.clone(), types.into_iter().map(str::to_owned).collect());
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
