//! Task 2 (issue #43): the reasoner-level `dropped_axioms` accessor +
//! a graceful-degradation integration test.
//!
//! `HasKey` is the live "was-aborting" exemplar (see
//! `owl_dl_core::convert::convert_component`'s `C::HasKey(_) =>
//! Err(ConversionError::UnsupportedAxiom { kind: "HasKey" })` arm):
//! before Task 1, that `Err` propagated out of `convert_ontology` via
//! `?` and aborted the whole conversion — so an ontology containing
//! a `HasKey` axiom alongside an otherwise-fully-supported `SubClassOf`
//! could not be classified at all. Task 1 made `convert_ontology`
//! catch per-component conversion errors and record them in
//! `InternalOntology.dropped` (a sound under-approximation) instead of
//! propagating, so `classify`/`is_consistent`/`realize` now succeed on
//! such inputs. This test pins that behaviour end-to-end through the
//! public reasoner API and exercises the new `dropped_axioms` accessor.

#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::{classify, dropped_axioms, is_consistent};
use std::io::Cursor;

/// `:A ⊑ :B` (supported) plus `HasKey(:A (:r) ())` (unsupported —
/// deferred advanced feature; see `convert_component`'s `HasKey` arm).
/// Mirrors the exact OFN shape already proven to parse in
/// `owl_dl_core::convert::tests::convert_records_dropped_unsupported_axiom_and_continues`.
const HAS_KEY_SRC: &str = r"Prefix(:=<http://ex/#>)
      Ontology(<http://ex/>
        Declaration(Class(:A)) Declaration(Class(:B))
        Declaration(ObjectProperty(:r))
        SubClassOf(:A :B)
        HasKey(:A (:r) ()))";

fn parse(src: &str) -> SetOntology<RcStr> {
    let mut reader = Cursor::new(src);
    let (onto, _) = read_ofn(&mut reader, ParserConfiguration::default()).expect("parse ofn");
    onto
}

#[test]
fn classify_succeeds_and_yields_subsumption_despite_unsupported_haskey() {
    let onto = parse(HAS_KEY_SRC);

    // RED (verified before the fix): this used to be `Err(ReasonError::Conversion(
    // UnsupportedAxiom { kind: "HasKey" }))` because the single unsupported
    // `HasKey` component aborted the entire `convert_ontology` call. GREEN
    // (post Task 1): conversion degrades gracefully, so `classify` succeeds.
    let classification =
        classify(&onto).expect("classify must not abort on a dropped HasKey axiom");

    assert!(
        classification.is_subclass("http://ex/#A", "http://ex/#B"),
        "the supported SubClassOf(:A :B) axiom must still be reflected in the hierarchy"
    );
}

#[test]
fn is_consistent_succeeds_despite_unsupported_haskey() {
    let onto = parse(HAS_KEY_SRC);
    let consistent =
        is_consistent(&onto).expect("is_consistent must not abort on a dropped HasKey axiom");
    assert!(consistent, "the ontology has a trivial model");
}

#[test]
fn dropped_axioms_reports_the_haskey_drop() {
    let onto = parse(HAS_KEY_SRC);
    let dropped = dropped_axioms(&onto).unwrap();

    assert!(
        dropped.total() >= 1,
        "at least the HasKey axiom must be recorded as dropped"
    );
    assert!(
        dropped.by_kind().keys().any(|k| k.contains("HasKey")),
        "dropped kinds should mention HasKey, got {:?}",
        dropped.by_kind()
    );
}

#[test]
fn dropped_axioms_is_empty_for_a_fully_supported_ontology() {
    let onto = parse(
        r"Prefix(:=<http://ex/#>)
          Ontology(<http://ex/>
            Declaration(Class(:A)) Declaration(Class(:B))
            SubClassOf(:A :B))",
    );
    let dropped = dropped_axioms(&onto).unwrap();
    assert!(
        dropped.is_empty(),
        "nothing should be dropped, got {:?}",
        dropped.by_kind()
    );
    assert_eq!(dropped.total(), 0);
}
