//! End-to-end driver tests for SP1.1 Layer B: label-driven same-tier sweep.
//!
//! The default `classify` groups classes into tiers by EL-closure-subsumer
//! count and assumes same-tier classes are mutually incomparable. Engine-
//! derived subsumptions between same-tier classes (e.g. inverse-domain
//! triggering on a generated successor) are missed by the tier walk alone
//! and are recovered by the defined-sup sweep (defined classes only) and,
//! after Layer B, by the label-driven sweep (any class appearing in some
//! `LabelOracle::Sat` label set).
//!
//! Run: `cargo test -p owl-dl-reasoner --test classify_inverse_domain`

#![allow(clippy::unwrap_used, clippy::doc_markdown)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::Classification;
use std::io::Cursor;

/// Mirror of the accessor used in `konclude_closure_diff.rs`.
/// Returns `true` iff the classification records `sub_iri ⊑ sup_iri`.
fn classification_has_subsumption(c: &Classification, sub_iri: &str, sup_iri: &str) -> bool {
    c.is_subclass(sub_iri, sup_iri)
}

/// SP1.1 Layer B driver: the inverse-domain chain that produces C ⊑ D.
///
/// Ontology:
/// ```text
///   C ⊑ ∃p.G
///   Domain(p⁻, H)          -- every domain of p⁻-edge (= target of p) gets :H
///   G ⊓ H ⊑ K
///   ∃p.K ⊑ D
/// ```
/// Because the wedge clausifier fires `Domain(p⁻, H)` at the generated
/// `p`-successor (the `G`-labelled node), that node gets `H`, hence `K`,
/// hence the whole chain yields `C ⊑ D`.  The classes `C` and `D` end up in
/// the same EL-closure tier; the tier walk misses the subsumption; Layer B's
/// label-driven sweep recovers it.
///
/// Before Layer B this test is RED; after it is GREEN.
#[test]
fn default_classify_finds_inverse_domain_subsumption() {
    let src = r"Prefix(:=<http://e#>)
Ontology(
Declaration(ObjectProperty(:p))
Declaration(Class(:C)) Declaration(Class(:G)) Declaration(Class(:H)) Declaration(Class(:K)) Declaration(Class(:D))
SubClassOf(:C ObjectSomeValuesFrom(:p :G))
ObjectPropertyDomain(ObjectInverseOf(:p) :H)
SubClassOf(ObjectIntersectionOf(:G :H) :K)
SubClassOf(ObjectSomeValuesFrom(:p :K) :D)
)
";
    let mut r = Cursor::new(src);
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut r, ParserConfiguration::default()).expect("parse");
    let c = owl_dl_reasoner::classify(&onto).expect("classify");
    assert!(
        classification_has_subsumption(&c, "http://e#C", "http://e#D"),
        "default classify must report C ⊑ D (inverse-domain on generated successor)"
    );
}

/// FP control: two sibling classes that both have a `p`-successor labelled `:E`
/// are NOT subsumption-related. Layer B must not add a spurious C⊑D or D⊑C.
#[test]
fn default_classify_no_spurious_same_tier_subsumption() {
    let src = r"Prefix(:=<http://e#>)
Ontology(
Declaration(ObjectProperty(:p))
Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:E))
SubClassOf(:A ObjectSomeValuesFrom(:p :E))
SubClassOf(:B ObjectSomeValuesFrom(:p :E))
)
";
    let mut r = Cursor::new(src);
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut r, ParserConfiguration::default()).expect("parse");
    let c = owl_dl_reasoner::classify(&onto).expect("classify");
    assert!(
        !classification_has_subsumption(&c, "http://e#A", "http://e#B"),
        "A ⋢ B: sibling p-successor classes must not spuriously subsume each other"
    );
    assert!(
        !classification_has_subsumption(&c, "http://e#B", "http://e#A"),
        "B ⋢ A: sibling p-successor classes must not spuriously subsume each other"
    );
}
