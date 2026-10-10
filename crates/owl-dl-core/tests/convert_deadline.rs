//! `convert_ontology_with_deadline` (#162): conversion that stops at a deadline.

#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_core::convert::{convert_ontology, convert_ontology_with_deadline};
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
