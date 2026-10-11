//! `convert_ontology_with_deadline` (#162): conversion that stops at a deadline.

#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_core::convert::{
    convert_ontology, convert_ontology_cut_after_polls, convert_ontology_with_deadline,
};
use std::fmt::Write as _;
use std::io::Cursor;
use std::time::{Duration, Instant};

fn onto() -> SetOntology<RcStr> {
    let mut src = String::from("Prefix(:=<http://ex#>)\nOntology(\n");
    for i in 0..3000 {
        writeln!(src, "SubClassOf(:A{i} :B{i})").unwrap();
        writeln!(src, "DataPropertyAssertion(:d :x{i} \"v{i}\")").unwrap();
    }
    src.push(')');
    read_ofn(&mut Cursor::new(src), ParserConfiguration::default())
        .unwrap()
        .0
}

/// No deadline, or one far off: identical to `convert_ontology`.
#[test]
fn without_a_binding_deadline_conversion_is_unchanged() {
    let o = onto();
    let full = convert_ontology(&o).unwrap();
    for deadline in [None, Some(Instant::now() + Duration::from_secs(3600))] {
        let (got, cut) = convert_ontology_with_deadline(&o, deadline).unwrap();
        assert!(!cut);
        assert_eq!(got.axioms, full.axioms);
        assert_eq!(got.vocabulary.num_classes(), full.vocabulary.num_classes());
    }
}

/// A passed deadline stops the main loop at its first poll: nothing converted.
#[test]
fn a_passed_deadline_cuts_conversion() {
    let (got, cut) = convert_ontology_with_deadline(&onto(), Some(Instant::now())).unwrap();
    assert!(cut);
    assert!(got.axioms.is_empty(), "{} axioms", got.axioms.len());
}

/// The deadline is thread-local state; it must not leak into a later
/// conversion on the same thread.
#[test]
fn a_cut_does_not_leak_into_the_next_conversion() {
    let o = onto();
    let (_, cut) = convert_ontology_with_deadline(&o, Some(Instant::now())).unwrap();
    assert!(cut);
    let full = convert_ontology(&o).unwrap();
    let (again, cut) = convert_ontology_with_deadline(&o, None).unwrap();
    assert!(!cut);
    assert!(!full.axioms.is_empty());
    assert_eq!(again.axioms, full.axioms);
}

/// 1,500 plain axioms (two main-loop polls) plus `K` nested integer ranges
/// `C_i ⊑ ∃d.[0, i]`, whose `DKey`s give `seed_bucket` about K²/2 subsumptions to
/// emit across K outer iterations. No disjunctive antecedents, negations or long
/// chains, so no rewrite pass applies and every cut must be a plain subset.
const K: usize = 120;

fn ranges() -> SetOntology<RcStr> {
    let mut src = String::from(
        "Prefix(:=<http://ex#>)\nPrefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)\nOntology(\n",
    );
    for i in 0..1500 {
        writeln!(src, "SubClassOf(:A{i} :B{i})").unwrap();
    }
    for i in 0..K {
        writeln!(
            src,
            "SubClassOf(:C{i} DataSomeValuesFrom(:d DatatypeRestriction(xsd:integer \
             xsd:minInclusive \"0\"^^xsd:integer xsd:maxInclusive \"{i}\"^^xsd:integer)))"
        )
        .unwrap();
    }
    src.push(')');
    read_ofn(&mut Cursor::new(src), ParserConfiguration::default())
        .unwrap()
        .0
}

/// The main-loop poll: the first two polls pass (before the sort, and at
/// component 0), the third cuts at component 1,024.
#[test]
fn a_cut_can_land_inside_the_main_loop() {
    let (got, cut, _) = convert_ontology_cut_after_polls(&ranges(), 2).unwrap();
    assert!(cut);
    assert!(
        !got.axioms.is_empty() && got.axioms.len() <= 1024,
        "{} axioms",
        got.axioms.len()
    );
}

/// Every possible cut point: each result is a subset of the full conversion,
/// and the `seed_bucket` poll yields one distinct partial size per outer
/// iteration, so there are at least about K of them.
#[test]
fn every_cut_point_yields_a_subset_and_the_seeding_loop_is_cut() {
    let o = ranges();
    let mut full = convert_ontology(&o).unwrap().axioms;
    full.sort();
    let (uncut, cut, polls) = convert_ontology_cut_after_polls(&o, usize::MAX).unwrap();
    assert!(!cut);
    assert_eq!(uncut.axioms, full, "the hook alone changes nothing");
    let mut sizes = std::collections::BTreeSet::new();
    for n in 0..polls {
        let (got, cut, _) = convert_ontology_cut_after_polls(&o, n).unwrap();
        assert!(cut, "poll {n} of {polls} must cut");
        for ax in &got.axioms {
            assert!(
                full.binary_search(ax).is_ok(),
                "cut at poll {n} added {ax:?}"
            );
        }
        sizes.insert(got.axioms.len());
    }
    assert!(
        sizes.len() >= K - 10,
        "only {} distinct cut sizes",
        sizes.len()
    );
}
