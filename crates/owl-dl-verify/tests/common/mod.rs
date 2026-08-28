//! Shared test helper: parse an OFN string (inline or read from a fixture
//! file) into an `InternalOntology`. Used by both `tests/model.rs` and
//! `tests/evaluator.rs` — extracted here rather than duplicated a third time.

pub(crate) fn load(ofn: &str) -> owl_dl_core::InternalOntology {
    let (onto, _): (
        horned_owl::ontology::set::SetOntology<horned_owl::model::RcStr>,
        _,
    ) = horned_owl::io::ofn::reader::read(
        &mut ofn.as_bytes(),
        horned_owl::io::ParserConfiguration::default(),
    )
    .expect("parse fixture");
    owl_dl_core::convert_ontology(&onto).expect("convert fixture")
}
