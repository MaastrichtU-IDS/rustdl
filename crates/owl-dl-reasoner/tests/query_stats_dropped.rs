//! #552: `QueryStats::dropped_axioms` and `QueryStats::completeness_guaranteed`.
//!
//! The count is stamped once per query entry point, so every early return carries
//! it: the built-in `owl:Thing` / `owl:Nothing` shortcuts and the tautologies are the
//! rows a per-literal approach would miss (a `QueryStats::default()` reads 0, which
//! would read as "nothing dropped"). `xsd:dateTimeStamp` is unsupported, so the two
//! axioms that mention it are dropped and the rest is pure EL; the `xsd:string`
//! control drops nothing.
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;

const C: &str = "http://e#C";
const E: &str = "http://e#E";
const THING: &str = "http://www.w3.org/2002/07/owl#Thing";
const NOTHING: &str = "http://www.w3.org/2002/07/owl#Nothing";

fn onto(range: &str) -> SetOntology<RcStr> {
    let src = format!(
        "Prefix(:=<http://e#>)\nPrefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)\nOntology(\n\
         Declaration(Class(:C)) Declaration(Class(:E)) Declaration(DataProperty(:p))\n\
         SubClassOf(:C DataSomeValuesFrom(:p {range}))\n\
         SubClassOf(DataSomeValuesFrom(:p {range}) :E)\n)"
    );
    read_ofn(&mut Cursor::new(src), ParserConfiguration::default())
        .expect("parse")
        .0
}

#[test]
fn every_single_query_path_reports_the_drop() {
    for (range, want) in [("xsd:dateTimeStamp", 2), ("xsd:string", 0)] {
        let o = onto(range);
        assert_eq!(
            owl_dl_reasoner::dropped_axioms(&o).unwrap().total(),
            want,
            "{range}: precondition"
        );
        let sub = |a: &str, b: &str| owl_dl_reasoner::is_subclass_of_with_stats(&o, a, b).unwrap();
        let sat = |a: &str| owl_dl_reasoner::is_class_satisfiable_with_stats(&o, a).unwrap();
        let rows = [
            ("subclass C ⊑ E (pure EL)", sub(C, E).1),
            ("subclass E ⊑ C (pure-EL no)", sub(E, C).1),
            ("subclass C ⊑ C (tautology)", sub(C, C).1),
            ("subclass C ⊑ owl:Thing", sub(C, THING).1),
            ("subclass owl:Nothing ⊑ C", sub(NOTHING, C).1),
            ("subclass C ⊑ owl:Nothing", sub(C, NOTHING).1),
            ("subclass owl:Thing ⊑ C", sub(THING, C).1),
            ("sat C", sat(C).1),
            ("sat owl:Thing (consistency)", sat(THING).1),
            ("sat owl:Nothing", sat(NOTHING).1),
            (
                "consistent",
                owl_dl_reasoner::is_consistent_with_stats(&o).unwrap().1,
            ),
        ];
        for (what, stats) in rows {
            assert_eq!(stats.dropped_axioms, want, "{range}: {what}");
        }
    }
}

/// The shared predicate: the pure-EL "no" is guaranteed only without the drop.
#[test]
fn the_shared_predicate_reads_the_drop() {
    let (holds, stats) =
        owl_dl_reasoner::is_subclass_of_with_stats(&onto("xsd:dateTimeStamp"), C, E).unwrap();
    assert!(!holds, "precondition: C ⊑ E is missed");
    assert!(stats.pure_el_mode, "precondition: the pure-EL fast path");
    assert!(
        !stats.completeness_guaranteed(),
        "a drop voids the guarantee"
    );

    let (holds, stats) =
        owl_dl_reasoner::is_subclass_of_with_stats(&onto("xsd:string"), E, C).unwrap();
    assert!(!holds && stats.pure_el_mode, "control precondition");
    assert!(
        stats.completeness_guaranteed(),
        "control: a complete pure-EL no"
    );
}
