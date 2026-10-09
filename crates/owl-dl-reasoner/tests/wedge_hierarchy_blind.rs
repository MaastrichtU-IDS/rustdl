//! #214: with Layer A off the wedge matches role ids exactly, and its trusted
//! `Sat` can silently miss an entailment that needs role-hierarchy reasoning.
//! Such a run now sets `wedge_hierarchy_blind` when the role hierarchy is
//! non-trivial, and not when there is no hierarchy to miss. It is a separate
//! signal: no deadline fires, so `timed_out_pairs` stays 0.
//!
//! In its own file because it sets `RUSTDL_CLASSIFY_ROLE_HIERARCHY=0`, which
//! sibling tests in one binary would observe.

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use std::io::Cursor;

fn classify(axioms: &str) -> owl_dl_reasoner::Classification {
    let ofn = format!(
        "Prefix(:=<http://ex.org/>)\nOntology(<http://ex.org/t>\n\
         Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:D))\n\
         Declaration(ObjectProperty(:r)) Declaration(ObjectProperty(:s))\n\
         Declaration(ObjectProperty(:t)) Declaration(ObjectProperty(:q))\n\
         Declaration(ObjectProperty(:v))\n{axioms}\n)\n"
    );
    let (o, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut Cursor::new(ofn), ParserConfiguration::default()).expect("parse");
    owl_dl_reasoner::classify(&o).expect("classify")
}

#[test]
#[allow(unsafe_code)]
fn a_hierarchy_blind_wedge_flags_the_result_and_a_plain_one_does_not() {
    // SAFETY: set_var is unsafe under edition 2024; this binary has one test.
    unsafe { std::env::set_var("RUSTDL_CLASSIFY_ROLE_HIERARCHY", "0") };

    // #214's probe t9 (HermiT: `A ≡ Nothing`). `s ⊑ r`, `t ⊑ q`, `r`/`q`
    // disjoint, and both under `v` with `≤1 v`: the `s`- and `t`-successors must
    // merge, which the disjointness forbids. Without Layer A the wedge sees
    // none of the sub-roles.
    let c = classify(
        "DisjointObjectProperties(:r :q) SubObjectPropertyOf(:s :r) \
         SubObjectPropertyOf(:t :q) SubObjectPropertyOf(:s :v) SubObjectPropertyOf(:t :v) \
         SubClassOf(:A ObjectIntersectionOf(ObjectSomeValuesFrom(:s :B) \
         ObjectSomeValuesFrom(:t :B) ObjectMaxCardinality(1 :v)))",
    );
    assert!(
        !c.unsatisfiable_classes().contains(&"http://ex.org/A"),
        "precondition: without Layer A the wedge misses this (else the test is vacuous)"
    );
    assert!(c.stats().wedge_hierarchy_blind);
    assert_eq!(c.stats().timed_out_pairs, 0, "no deadline fired");

    // No role axioms at all: nothing for the wedge to be blind to.
    let plain = classify(
        "SubClassOf(:B ObjectAllValuesFrom(:r :D)) SubClassOf(:A ObjectSomeValuesFrom(:r :D))",
    );
    assert!(!plain.stats().wedge_hierarchy_blind);
    assert_eq!(plain.stats().timed_out_pairs, 0);
}
