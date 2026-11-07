//! #457: `is_subclass_of` did not finish on an 8-axiom inconsistent `TBox`: its
//! H4 hypertableau wedge ran with no deadline. It now runs under
//! `RUSTDL_QUERY_WEDGE_MS` (default 3000) and falls through to the main tableau.
//!
//! The query runs on a thread; a regression fails at the receive timeout
//! instead of hanging the suite (the thread is left behind, so this test has a
//! file of its own).

#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use std::io::Cursor;
use std::time::Duration;

const HANG: &str = include_str!("../../owl-dl-cli/tests/fixtures/query_wedge_hang_457.ofn");

#[test]
fn is_subclass_of_finishes_on_the_wedge_hang_reproducer() {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let onto: SetOntology<RcStr> =
            read_ofn(&mut Cursor::new(HANG), ParserConfiguration::default())
                .unwrap()
                .0;
        let r = owl_dl_reasoner::is_subclass_of_with_stats(
            &onto,
            "http://gen.rustdl/o#C0",
            "http://gen.rustdl/o#C1",
        );
        tx.send(r.map(|(holds, _)| holds)).ok();
    });
    let answer = rx
        .recv_timeout(Duration::from_secs(60))
        .expect("is_subclass_of did not finish within 60 s");
    // The KB is inconsistent and has no individual: since #457's second residual
    // the tableau-answered query runs the bounded main-tableau consistency check
    // (`RUSTDL_QUERY_NO_SEED_TAIL`), so it errors instead of answering a vacuous
    // `true`.
    assert!(
        matches!(answer, Err(owl_dl_reasoner::ReasonError::Inconsistent)),
        "{answer:?}"
    );
}
