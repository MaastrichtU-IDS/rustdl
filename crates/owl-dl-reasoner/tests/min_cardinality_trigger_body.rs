//! A `≥n R.X` (n ≥ 2) inside a trigger body must not be read as `∃R.X`.
//!
//! The saturator's body lowerings under-approximate `≥n R.X` as `∃R.X`. That is sound where
//! the concept is asserted (a weaker fact), but a trigger body is a SUFFICIENT condition. So
//! `H ≡ ∃r.(≥2 s.X)` fired on `Y ⊑ ∃r.∃s.X` and `classify` reported `Y ⊑ H`. Konclude and
//! HermiT both refute it: a `Y` whose `r`-successor has one `s`-successor is a countermodel.
//! The `≥1` control is the discriminator: `≥1 s.X` IS `∃s.X`, so there `Y ⊑ H` holds and
//! Konclude derives it. The fix keys `≥n R.X` (`n ≥ 2`) as its own class, so the SAME
//! restriction asserted on both sides still matches: the ChEBI-style
//! `SulfonicAcidGroup ⊑ SulfonicAcidDerivativeGroup` (`ore_ont_10019`, `1707`, `6333`, `6485`,
//! Konclude-confirmed) depends on that, and refusing `≥2` bodies outright lost it.

#![allow(clippy::unwrap_used, clippy::doc_markdown)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_core::convert::convert_ontology;
use std::io::Cursor;

fn parse(ont: &str) -> SetOntology<RcStr> {
    let (o, _): (SetOntology<RcStr>, _) = read_ofn(
        &mut Cursor::new(ont.to_string()),
        ParserConfiguration::default(),
    )
    .expect("parse");
    o
}

fn sat_sub(ont: &str, x: &str, y: &str) -> bool {
    let internal = convert_ontology(&parse(ont)).expect("lower");
    let subsumers = owl_dl_saturation::saturate(&internal);
    let xid = internal.vocabulary.class_id(x).expect("x");
    let yid = internal.vocabulary.class_id(y).expect("y");
    subsumers.contains(xid, yid)
}

fn classify_sub(ont: &str, x: &str, y: &str) -> bool {
    owl_dl_reasoner::classify(&parse(ont))
        .expect("classify")
        .is_subclass(x, y)
}

/// `H ≡ ∃r.<body>`, `Y ⊑ ∃r.∃s.X`.
fn ont(body: &str) -> String {
    format!(
        r"Prefix(:=<http://e/>)
Ontology(<http://e/o>
Declaration(Class(:H)) Declaration(Class(:Y)) Declaration(Class(:X))
Declaration(ObjectProperty(:r)) Declaration(ObjectProperty(:s)) Declaration(ObjectProperty(:R))
EquivalentClasses(:H ObjectSomeValuesFrom(:r {body}))
SubClassOf(:Y ObjectSomeValuesFrom(:r ObjectSomeValuesFrom(:s :X)))
)"
    )
}

const Y: &str = "http://e/Y";
const H: &str = "http://e/H";

#[test]
fn min_two_in_a_definition_is_not_read_as_an_existential() {
    let o = ont("ObjectMinCardinality(2 :s :X)");
    assert!(!sat_sub(&o, Y, H), "saturation: Y ⋢ H");
    assert!(!classify_sub(&o, Y, H), "classify: Y ⋢ H");
}

#[test]
fn min_one_is_an_existential_and_still_derives_the_subsumption() {
    // Discriminating control: `≥1 s.X ≡ ∃s.X`, so `Y ⊑ H` is entailed (Konclude agrees).
    let o = ont("ObjectMinCardinality(1 :s :X)");
    assert!(sat_sub(&o, Y, H), "saturation: Y ⊑ H");
    assert!(classify_sub(&o, Y, H), "classify: Y ⊑ H");
}

#[test]
fn min_two_nested_in_a_conjunction_does_not_match_an_existential() {
    let o = ont("ObjectIntersectionOf(:X ObjectMinCardinality(2 :s :X))");
    let o = o.replace(
        "SubClassOf(:Y ObjectSomeValuesFrom(:r ObjectSomeValuesFrom(:s :X)))",
        "SubClassOf(:Y ObjectSomeValuesFrom(:r ObjectIntersectionOf(:X ObjectSomeValuesFrom(:s :X))))",
    );
    assert!(!sat_sub(&o, Y, H));
    assert!(!classify_sub(&o, Y, H));
}

#[test]
fn min_two_under_a_forall_does_not_match_an_existential() {
    // `∀R.(≥2 s.X)` in a definition vs `∀R.∃s.X` asserted: the same false positive one level
    // down. Not reachable at default settings (the saturator drops a `∀` inside a filler).
    let o = ont("ObjectAllValuesFrom(:R ObjectMinCardinality(2 :s :X))").replace(
        "SubClassOf(:Y ObjectSomeValuesFrom(:r ObjectSomeValuesFrom(:s :X)))",
        "SubClassOf(:Y ObjectSomeValuesFrom(:r ObjectAllValuesFrom(:R ObjectSomeValuesFrom(:s :X))))",
    );
    assert!(!classify_sub(&o, Y, H));
}

#[test]
fn an_asserted_min_two_still_entails_the_weaker_existential() {
    // The necessary direction stays: `Y ⊑ ∃r.(≥2 s.X)` does entail `Y ⊑ ∃r.∃s.X`.
    let o = r"Prefix(:=<http://e/>)
Ontology(<http://e/o>
Declaration(Class(:H)) Declaration(Class(:Y)) Declaration(Class(:X))
Declaration(ObjectProperty(:r)) Declaration(ObjectProperty(:s))
EquivalentClasses(:H ObjectSomeValuesFrom(:r ObjectSomeValuesFrom(:s :X)))
SubClassOf(:Y ObjectSomeValuesFrom(:r ObjectMinCardinality(2 :s :X)))
)";
    assert!(sat_sub(o, Y, H), "saturation: Y ⊑ H");
    assert!(classify_sub(o, Y, H), "classify: Y ⊑ H");
}

#[test]
fn the_same_min_two_on_both_sides_still_matches() {
    // `H ≡ ∃r.(≥2 s.X)`, `Y ⊑ ∃r.(≥2 s.X)`: entailed, and derived by saturation through the key.
    let o = ont("ObjectMinCardinality(2 :s :X)").replace(
        "SubClassOf(:Y ObjectSomeValuesFrom(:r ObjectSomeValuesFrom(:s :X)))",
        "SubClassOf(:Y ObjectSomeValuesFrom(:r ObjectMinCardinality(2 :s :X)))",
    );
    assert!(sat_sub(&o, Y, H), "saturation: Y ⊑ H");
    assert!(classify_sub(&o, Y, H), "classify: Y ⊑ H");
}
