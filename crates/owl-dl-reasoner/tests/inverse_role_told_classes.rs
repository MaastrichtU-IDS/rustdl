//! #526: `realize::told_classes_of` gave an inverse-role assertion's domain to its
//! subject. `ObjectPropertyAssertion(ObjectInverseOf(:r) :a :b)` is the edge
//! `r(b, a)`, so `Domain(r) = D` types `b` and `Range(r) = R` types `a`; the told
//! path typed `a : D` and `b : R`, both false. It feeds the instance check's told
//! fast path (a `yes` without the tableau probe) and is the sole decider on the
//! saturation-only surfaces.
//!
//! Negatives first. Every expectation here was adjudicated by `HermiT` and Konclude
//! (`ClassAssertion(C x)` entailed or refuted by both).
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::{
    instances_of, instances_of_saturation_only, is_instance_of, is_instance_of_saturation_only,
    realize, realize_saturation_only,
};

const NS: &str = "http://ex/#";

fn onto(declared: bool, body: &str) -> SetOntology<RcStr> {
    let decls = if declared {
        "Declaration(ObjectProperty(:r)) Declaration(Class(:D)) Declaration(Class(:R))\n\
         Declaration(Class(:D2)) Declaration(Class(:R2))\n\
         Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))\n\
         Declaration(NamedIndividual(:c)) Declaration(NamedIndividual(:d))\n"
    } else {
        ""
    };
    let src = format!("Prefix(:=<http://ex/#>)\nOntology(<http://ex/o>\n{decls}{body}\n)\n");
    read_ofn(&mut Cursor::new(src), ParserConfiguration::default())
        .unwrap()
        .0
}

fn iri(local: &str) -> String {
    format!("{NS}{local}")
}

/// `x : C` on the full and the saturation-only instance checks. `sat_only` is the
/// saturation-only answer expected: the told path is its sole decider, so a missed
/// told class shows there as `false`.
#[track_caller]
fn check(o: &SetOntology<RcStr>, case: &str, class: &str, ind: &str, full: bool, sat_only: bool) {
    let (c, i) = (iri(class), iri(ind));
    assert_eq!(
        is_instance_of(o, &c, &i).unwrap(),
        full,
        "{case}: is_instance_of({class}, {ind})"
    );
    assert_eq!(
        is_instance_of_saturation_only(o, &c, &i).unwrap(),
        sat_only,
        "{case}: is_instance_of_saturation_only({class}, {ind})"
    );
}

#[track_caller]
fn instances(o: &SetOntology<RcStr>, case: &str, class: &str, want: &[&str]) {
    let want: Vec<String> = want.iter().map(|l| iri(l)).collect();
    let mut full = instances_of(o, &iri(class)).unwrap();
    full.sort();
    assert_eq!(full, want, "{case}: instances_of({class})");
    let mut sat = instances_of_saturation_only(o, &iri(class)).unwrap();
    sat.sort();
    assert_eq!(sat, want, "{case}: instances_of_saturation_only({class})");
}

/// `entailed_types(ind)` contains every class in `has` and none in `lacks`, on
/// `realize` and `realize_saturation_only`.
#[track_caller]
fn types(o: &SetOntology<RcStr>, case: &str, ind: &str, has: &[&str], lacks: &[&str]) {
    for (surface, r) in [
        ("realize", realize(o).unwrap()),
        (
            "realize_saturation_only",
            realize_saturation_only(o).unwrap(),
        ),
    ] {
        let got = r.entailed_types(&iri(ind));
        for l in lacks {
            assert!(
                !got.contains(&iri(l)),
                "{case}: {surface} types {ind} as {l}: {got:?}"
            );
        }
        for h in has {
            assert!(
                got.contains(&iri(h)),
                "{case}: {surface} misses {ind} : {h}: {got:?}"
            );
        }
    }
}

const DOM_RNG: &str = "ObjectPropertyDomain(:r :D) ObjectPropertyRange(:r :R)";

/// The #526 fixture, declared and undeclared.
#[test]
fn an_inverse_assertion_types_the_named_edge_s_ends() {
    for declared in [true, false] {
        let case = if declared { "declared" } else { "undeclared" };
        let o = onto(
            declared,
            &format!("{DOM_RNG} ObjectPropertyAssertion(ObjectInverseOf(:r) :a :b)"),
        );
        check(&o, case, "D", "a", false, false);
        check(&o, case, "R", "b", false, false);
        check(&o, case, "D", "b", true, true);
        check(&o, case, "R", "a", true, true);
        instances(&o, case, "D", &["b"]);
        instances(&o, case, "R", &["a"]);
        types(&o, case, "a", &["R"], &["D"]);
        types(&o, case, "b", &["D"], &["R"]);
    }
}

/// Domain only and range only: each arm of the swap on its own.
#[test]
fn each_end_of_the_swap_alone() {
    let o = onto(
        true,
        "ObjectPropertyDomain(:r :D) ObjectPropertyAssertion(ObjectInverseOf(:r) :a :b)",
    );
    check(&o, "domain only", "D", "a", false, false);
    check(&o, "domain only", "D", "b", true, true);
    let o = onto(
        true,
        "ObjectPropertyRange(:r :R) ObjectPropertyAssertion(ObjectInverseOf(:r) :a :b)",
    );
    check(&o, "range only", "R", "b", false, false);
    check(&o, "range only", "R", "a", true, true);
}

/// A named and an inverse assertion on the same role: `r(a, b)` and `r⁻(c, d)`.
#[test]
fn named_and_inverse_assertions_on_one_role() {
    let o = onto(
        true,
        &format!(
            "{DOM_RNG} ObjectPropertyAssertion(:r :a :b) \
             ObjectPropertyAssertion(ObjectInverseOf(:r) :c :d)"
        ),
    );
    for (class, ind) in [("R", "a"), ("D", "b"), ("D", "c"), ("R", "d")] {
        check(&o, "mixed", class, ind, false, false);
    }
    for (class, ind) in [("D", "a"), ("R", "b"), ("R", "c"), ("D", "d")] {
        check(&o, "mixed", class, ind, true, true);
    }
    instances(&o, "mixed", "D", &["a", "d"]);
    instances(&o, "mixed", "R", &["b", "c"]);
    // On main, `realize` typed `c` as both `D` and `R`.
    types(&o, "mixed", "c", &["R"], &["D"]);
    types(&o, "mixed", "d", &["D"], &["R"]);
}

/// `r⁻(a, a)` is `r(a, a)`: both ends are `a`. Main gave the same answers, so this pins
/// that the told path keeps inverse self-loops (sabotage S3), not the swap itself.
#[test]
fn an_inverse_self_loop_gets_both_ends() {
    let o = onto(
        true,
        &format!("{DOM_RNG} ObjectPropertyAssertion(ObjectInverseOf(:r) :a :a)"),
    );
    check(&o, "self-loop", "D", "a", true, true);
    check(&o, "self-loop", "R", "a", true, true);
    check(&o, "self-loop", "D", "b", false, false);
}

/// The told path reads named-role domain and range axioms only: `Domain(r⁻) = D2`
/// is `Range(r) = D2`, and must not type the source of an `r`-edge. On the
/// saturation-only path the entailed `b : D2` and `a : R2` are sound misses; the
/// full check reaches them through the tableau probe.
#[test]
fn inverse_role_domain_axioms_never_type_the_edge_s_source() {
    let o = onto(
        true,
        "ObjectPropertyDomain(ObjectInverseOf(:r) :D2) ObjectPropertyRange(ObjectInverseOf(:r) :R2) \
         ObjectPropertyAssertion(:r :a :b)",
    );
    check(&o, "Domain(r⁻)", "D2", "a", false, false);
    check(&o, "Range(r⁻)", "R2", "b", false, false);
    assert!(
        is_instance_of(&o, &iri("D2"), &iri("b")).unwrap(),
        "Domain(r⁻): b : D2"
    );
    assert!(
        is_instance_of(&o, &iri("R2"), &iri("a")).unwrap(),
        "Range(r⁻): a : R2"
    );
}
