//! #290: `InverseFunctional(G)` with the assertions on a SUB-role of `G` or `G⁻`.
//!
//! `S ⊑ G⁻`, `a S b`, `a S c` give `b G a` and `c G a`, so inverse-functional `G`
//! forces `b = c`, and `DifferentIndividuals(b, c)` makes the KB inconsistent.
//! `rustdl consistent` and `classify` both said consistent. The `≤1 G⁻` bound that
//! drives the merge is emitted only when `inv_func_merge_consumable` admits it, and
//! that gate's assertion arm checked the exact role id `G`, never its sub-roles.
//! `FunctionalObjectProperty(ObjectInverseOf(G))`, the same axiom spelled the other
//! way, takes the unconditional functional route and was always detected.
//!
//! `HermiT` agrees with every verdict below (2026-10-19), including the two
//! consistent controls.
//!
//! Run: `cargo test -p owl-dl-reasoner --test inverse_functional_sub_role_assertions`

#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use std::io::Cursor;

fn parse(body: &str) -> SetOntology<RcStr> {
    let src = format!(
        "Prefix(:=<http://t/>)\nOntology(<http://t/o>\n\
         Declaration(ObjectProperty(:G)) Declaration(ObjectProperty(:S))\n\
         Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))\n\
         Declaration(NamedIndividual(:c))\n{body}\n)\n"
    );
    let (o, _) = read_ofn(&mut Cursor::new(src), ParserConfiguration::default()).expect("parse");
    o
}

fn consistent(body: &str) -> bool {
    owl_dl_reasoner::is_consistent(&parse(body)).expect("consistency")
}

fn classify_consistent(body: &str) -> bool {
    !owl_dl_reasoner::classify(&parse(body))
        .expect("classify")
        .stats()
        .inconsistent
}

const SUB_OF_INVERSE: &str = "    InverseFunctionalObjectProperty(:G)
    SubObjectPropertyOf(:S ObjectInverseOf(:G))
    ObjectPropertyAssertion(:S :a :b) ObjectPropertyAssertion(:S :a :c)
    DifferentIndividuals(:b :c)";

#[test]
fn a_sub_role_of_the_inverse_forces_the_merge() {
    assert!(!consistent(SUB_OF_INVERSE));
    assert!(!classify_consistent(SUB_OF_INVERSE));
}

/// The same inclusion spelled `S⁻ ⊑ G`.
#[test]
fn the_inverse_spelling_of_the_inclusion_forces_the_merge() {
    let body = SUB_OF_INVERSE.replace(
        "SubObjectPropertyOf(:S ObjectInverseOf(:G))",
        "SubObjectPropertyOf(ObjectInverseOf(:S) :G)",
    );
    assert!(!consistent(&body));
}

/// A forward sub-role: `S ⊑ G`, `b S a`, `c S a` give `b G a`, `c G a`.
#[test]
fn a_forward_sub_role_forces_the_merge() {
    let body = "    InverseFunctionalObjectProperty(:G) SubObjectPropertyOf(:S :G)
    ObjectPropertyAssertion(:S :b :a) ObjectPropertyAssertion(:S :c :a)
    DifferentIndividuals(:b :c)";
    assert!(!consistent(body));
}

/// Control: without `DifferentIndividuals` the merge is harmless.
#[test]
fn without_different_individuals_it_stays_consistent() {
    let body = SUB_OF_INVERSE.replace("    DifferentIndividuals(:b :c)", "");
    assert!(consistent(&body));
}

/// Control: with `S ⊑ G⁻`, edges INTO `a` (`b S a`, `c S a`) are `a G b`, `a G c`:
/// two `G`-successors of one source, which inverse-functionality does not bound.
#[test]
fn edges_the_other_way_force_nothing() {
    let body = SUB_OF_INVERSE
        .replace(
            "ObjectPropertyAssertion(:S :a :b)",
            "ObjectPropertyAssertion(:S :b :a)",
        )
        .replace(
            "ObjectPropertyAssertion(:S :a :c)",
            "ObjectPropertyAssertion(:S :c :a)",
        );
    assert!(consistent(&body));
}
