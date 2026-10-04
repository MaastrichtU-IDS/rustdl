#![allow(clippy::doc_markdown)]
//! `ReflexiveObjectProperty(r)` was dropped from the wedge's clause theory: it
//! fell into `clausify_axiom`'s catch-all, uncounted. `Reflexive(r)` +
//! `Range(r, C)` entails `⊤ ⊑ C`, so every class is subsumed by `C`, but under
//! `trust_sat` the wedge's `Sat` (built without the self-loop) refuted every
//! `X ⊑ C` silently. Konclude reports `owl:Thing ≡ C` on this fixture.
//!
//! Found while sweeping #190 gap 2: removing a re-pick stall let the label
//! cache reach that wrong `Sat` on `ore_ont_10517`, where the stall had
//! previously routed those pairs to the main tableau.

use owl_dl_reasoner::classify;

const ONTOLOGY: &str = r"Prefix(:=<http://ex.org/#>)
Ontology(<http://ex.org/refl>
Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C)) Declaration(Class(:D))
Declaration(ObjectProperty(:r)) Declaration(ObjectProperty(:p))
ReflexiveObjectProperty(:r)
ObjectPropertyRange(:r :C)
SubClassOf(:A ObjectAllValuesFrom(:p :B))
SubClassOf(:D ObjectUnionOf(:A :B))
)
";

fn is_under_c(src: &str, sub: &str) -> bool {
    let mut cur = std::io::Cursor::new(src.to_owned());
    let (onto, _): (
        horned_owl::ontology::set::SetOntology<horned_owl::model::RcStr>,
        _,
    ) = horned_owl::io::ofn::reader::read(&mut cur, horned_owl::io::ParserConfiguration::default())
        .expect("parse");
    classify(&onto)
        .expect("classify")
        .is_subclass(&format!("http://ex.org/#{sub}"), "http://ex.org/#C")
}

#[test]
fn reflexive_role_with_a_range_subsumes_every_class_under_the_range() {
    for x in ["A", "B", "D"] {
        assert!(is_under_c(ONTOLOGY, x), "classify misses {x} ⊑ C");
    }
}

/// Negative control: without the reflexivity axiom nothing is under `C`, so
/// the positive test is not passing for an unrelated reason.
#[test]
fn without_reflexivity_nothing_is_under_the_range() {
    let src = ONTOLOGY.replace("ReflexiveObjectProperty(:r)\n", "");
    for x in ["A", "B", "D"] {
        assert!(
            !is_under_c(&src, x),
            "{x} ⊑ C must not hold without reflexivity"
        );
    }
}
