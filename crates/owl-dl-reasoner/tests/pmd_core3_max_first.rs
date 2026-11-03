//! Issue #288: `sat PMD_0010100` did not terminate on the full PMD core ontology,
//! or on this 15-axiom core of it ("core3"). #382's `RUSTDL_MAX_FIRST` and
//! `RUSTDL_TOP_FILLER` (default ON since #397) fixed both: core3 answers in
//! ~0.01 s, and `HermiT` calls the class satisfiable.
//!
//! Measured on 7da5cd85 in release: with both flags on, 0.00 CPU-s; with
//! `RUSTDL_MAX_FIRST=0 RUSTDL_TOP_FILLER=0`, 33.6 CPU-s; on the full ontology,
//! with both off, no answer at all. So the bound below, a few hundred times
//! the fixed cost, fails if either fix regresses.
//!
//! Sabotage, run by hand: `RUSTDL_MAX_FIRST=0 RUSTDL_TOP_FILLER=0 cargo test -p
//! owl-dl-reasoner --test pmd_core3_max_first` aborts after `BOUND`.
#![allow(clippy::unwrap_used)]

use std::io::{Cursor, Write};
use std::sync::mpsc;
use std::time::Duration;

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::is_class_satisfiable;

/// The fixed path answers in ~0.01 s (release); the broken one takes ~34 s.
const BOUND: Duration = Duration::from_secs(10);

/// core3 from the #288 investigation, with the original IRIs (rule order follows
/// class and role ids, so shortened IRIs may not reproduce the stall).
const CORE3: &str = r"Prefix(obo:=<http://purl.obolibrary.org/obo/>)
Prefix(pmd:=<https://w3id.org/pmd/co/>)
Ontology(
SubClassOf(pmd:PMD_0090002 pmd:PMD_0023004)
SubClassOf(pmd:PMD_0010100 pmd:PMD_0090000)
SubClassOf(pmd:PMD_0090002 ObjectSomeValuesFrom(obo:RO_0000080 pmd:PMD_0020293))
SubClassOf(pmd:PMD_0090000 ObjectSomeValuesFrom(obo:RO_0000086 pmd:PMD_0090002))
EquivalentClasses(pmd:PMD_0020318 ObjectIntersectionOf(ObjectComplementOf(ObjectSomeValuesFrom(obo:RO_0002328 pmd:PMD_0023000)) ObjectSomeValuesFrom(obo:RO_0000052 ObjectUnionOf(obo:CHEBI_59999 pmd:PMD_0000001))))
EquivalentClasses(pmd:PMD_0023004 ObjectUnionOf(ObjectSomeValuesFrom(obo:RO_0000052 pmd:PMD_0000001) ObjectSomeValuesFrom(pmd:PMD_0025999 pmd:PMD_0000001)))
EquivalentClasses(pmd:PMD_0023007 ObjectUnionOf(ObjectSomeValuesFrom(obo:RO_0000052 obo:BFO_0000027) ObjectSomeValuesFrom(pmd:PMD_0025999 obo:BFO_0000027)))
EquivalentClasses(pmd:PMD_0023023 ObjectUnionOf(ObjectSomeValuesFrom(obo:RO_0000052 pmd:PMD_0020113) ObjectSomeValuesFrom(pmd:PMD_0025999 pmd:PMD_0020113)))
EquivalentClasses(pmd:PMD_0023041 ObjectUnionOf(ObjectSomeValuesFrom(obo:RO_0000052 obo:BFO_0000029) ObjectSomeValuesFrom(pmd:PMD_0025999 obo:BFO_0000029)))
EquivalentClasses(pmd:PMD_0090000 ObjectSomeValuesFrom(obo:RO_0000091 pmd:PMD_0090001))
SubObjectPropertyOf(obo:RO_0000080 obo:RO_0000052)
SubObjectPropertyOf(obo:RO_0000092 obo:RO_0000052)
InverseObjectProperties(obo:RO_0000080 obo:RO_0000086)
InverseObjectProperties(obo:RO_0000091 obo:RO_0000092)
FunctionalObjectProperty(obo:RO_0000052)
)";

/// `PMD_0010100` is satisfiable on core3, and the answer comes within `BOUND`.
///
/// On a timeout this ABORTS rather than panics, so the stuck worker cannot keep
/// burning CPU under the rest of the test binary.
#[test]
fn core3_pmd_0010100_is_satisfiable_in_bounded_time() {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let o: SetOntology<RcStr> = read_ofn(
            &mut Cursor::new(CORE3.to_owned()),
            ParserConfiguration::default(),
        )
        .unwrap()
        .0;
        let _ = tx.send(is_class_satisfiable(
            &o,
            "https://w3id.org/pmd/co/PMD_0010100",
        ));
    });
    match rx.recv_timeout(BOUND) {
        Ok(r) => assert!(r.unwrap(), "PMD_0010100 must be satisfiable (HermiT)"),
        Err(mpsc::RecvTimeoutError::Disconnected) => panic!("the worker panicked"),
        Err(mpsc::RecvTimeoutError::Timeout) => {
            let _ = writeln!(
                std::io::stderr(),
                "sat PMD_0010100 on core3 did not finish within {BOUND:?} — issue #288 has regressed"
            );
            std::process::abort();
        }
    }
}
