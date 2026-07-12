//! Syntactic ontology profiling: signature counts computed by a single
//! structural walk of the parsed ontology.
//!
//! This performs **no reasoning**. Unlike a classification, a syntactic walk
//! can never diverge or hang — which is exactly what corpus staging needs.
//! The class count previously came from `classify(&onto).classes().len()`,
//! an *unbounded* full classification run purely for a metadata number; on a
//! hard ontology (the ORE-2015 pilot `ore_ont_10019`) that classification's
//! satisfiability probe exploded combinatorially with no deadline and pegged
//! every core forever, wedging the whole matrix run on the first ontology.
//! A class *count* is a syntactic property of the signature and needs none of
//! that.

use horned_owl::model::{Class, DataProperty, ForIRI, NamedIndividual, ObjectProperty};
use horned_owl::ontology::set::SetOntology;
use horned_owl::visitor::immutable::{Visit, Walk};
use std::collections::HashSet;

/// `owl:Thing` / `owl:Nothing` are excluded from the class count so a
/// signature count lines up with how reasoners report "number of classes".
const OWL_THING: &str = "http://www.w3.org/2002/07/owl#Thing";
const OWL_NOTHING: &str = "http://www.w3.org/2002/07/owl#Nothing";

/// Distinct named entities appearing anywhere in an ontology's signature.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct SyntacticProfile {
    /// Distinct named classes (excluding `owl:Thing` / `owl:Nothing`).
    pub classes: usize,
    /// Distinct named object properties.
    pub object_properties: usize,
    /// Distinct named data properties.
    pub data_properties: usize,
    /// Distinct named individuals.
    pub named_individuals: usize,
}

/// A [`Visit`] that accumulates the distinct IRIs of each named-entity kind.
/// Every reference is visited (declarations *and* uses inside expressions), so
/// the collected sets are the full signature, not just declared entities.
#[derive(Default)]
struct SignatureCollector {
    classes: HashSet<String>,
    object_properties: HashSet<String>,
    data_properties: HashSet<String>,
    named_individuals: HashSet<String>,
}

impl<A: ForIRI> Visit<A> for SignatureCollector {
    fn visit_class(&mut self, c: &Class<A>) {
        let iri = c.0.to_string();
        if iri != OWL_THING && iri != OWL_NOTHING {
            self.classes.insert(iri);
        }
    }

    fn visit_object_property(&mut self, p: &ObjectProperty<A>) {
        self.object_properties.insert(p.0.to_string());
    }

    fn visit_data_property(&mut self, p: &DataProperty<A>) {
        self.data_properties.insert(p.0.to_string());
    }

    fn visit_named_individual(&mut self, i: &NamedIndividual<A>) {
        self.named_individuals.insert(i.0.to_string());
    }
}

/// Walk `onto` once and return its syntactic signature profile.
///
/// Cost is linear in the size of the ontology; it performs no reasoning and
/// cannot hang.
pub fn profile<A: ForIRI>(onto: &SetOntology<A>) -> SyntacticProfile {
    let mut walk = Walk::new(SignatureCollector::default());
    for ac in onto {
        walk.annotated_component(ac);
    }
    let sig = walk.into_visit();
    SyntacticProfile {
        classes: sig.classes.len(),
        object_properties: sig.object_properties.len(),
        data_properties: sig.data_properties.len(),
        named_individuals: sig.named_individuals.len(),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use horned_owl::io::ParserConfiguration;
    use horned_owl::model::RcStr;

    fn parse(ofn: &str) -> SetOntology<RcStr> {
        let mut cur = std::io::Cursor::new(ofn.to_string());
        let (onto, _): (SetOntology<RcStr>, _) =
            horned_owl::io::ofn::reader::read(&mut cur, ParserConfiguration::default()).unwrap();
        onto
    }

    #[test]
    fn counts_distinct_classes_from_declarations_and_uses() {
        // :A and :B are declared; :C appears only inside an axiom. All three
        // are named classes; owl:Thing (implicit) must not be counted.
        let onto = parse(
            "Prefix(:=<http://e/>)\n\
             Ontology(\n\
               Declaration(Class(:A))\n\
               Declaration(Class(:B))\n\
               SubClassOf(:A ObjectSomeValuesFrom(:r :C))\n\
             )\n",
        );
        let p = profile(&onto);
        assert_eq!(p.classes, 3, "expected :A :B :C");
        assert_eq!(p.object_properties, 1, "expected :r");
    }

    #[test]
    fn excludes_owl_thing_and_nothing() {
        let onto = parse(
            "Prefix(:=<http://e/>)\n\
             Prefix(owl:=<http://www.w3.org/2002/07/owl#>)\n\
             Ontology(\n\
               Declaration(Class(:A))\n\
               SubClassOf(:A owl:Thing)\n\
               SubClassOf(owl:Nothing :A)\n\
             )\n",
        );
        let p = profile(&onto);
        assert_eq!(p.classes, 1, "only :A; owl:Thing/owl:Nothing excluded");
    }

    #[test]
    fn empty_ontology_is_all_zero() {
        let onto = parse("Prefix(:=<http://e/>)\nOntology()\n");
        assert_eq!(profile(&onto), SyntacticProfile::default());
    }
}
