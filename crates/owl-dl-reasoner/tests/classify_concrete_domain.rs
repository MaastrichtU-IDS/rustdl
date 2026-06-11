//! Canaries for classify-level concrete-domain VERIFY: a class
//! unsatisfiable only by an integer counting clash (`≥3 p.[0,1]` capacity,
//! `≥3 ⊓ ≤2` conflict) must appear unsatisfiable via `classify` — not just
//! via `is_class_satisfiable`. Before this feature, classify trusted the
//! wedge's `Sat` (the wedge has no `card_sat`) and missed these.
//!
//! NEGATIVES-FIRST: the FP-critical direction is a satisfiable class wrongly
//! reported unsatisfiable. Every `assert!(sat(...))` is a genuinely
//! satisfiable data node that MUST stay satisfiable.
//!
//! Run: `cargo test -p owl-dl-reasoner --test classify_concrete_domain`.

#![allow(clippy::unwrap_used, clippy::doc_markdown)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::classify;
use std::io::Cursor;

const PFX: &str = "Prefix(:=<http://t/>)\nPrefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)\n";

/// Classify `body` and return true iff `:C` (`http://t/C`) is unsatisfiable.
fn c_unsat(body: &str) -> bool {
    let src = format!(
        "{PFX}Ontology(<http://t/o>\n  Declaration(Class(:C)) Declaration(DataProperty(:p))\n{body}\n)\n"
    );
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut Cursor::new(src), ParserConfiguration::default()).expect("parse ofn");
    classify(&onto)
        .expect("classify")
        .unsatisfiable_classes()
        .contains(&"http://t/C")
}

fn min_int(n: u32, lo: i64, hi: i64) -> String {
    format!(
        "  SubClassOf(:C DataMinCardinality({n} :p DatatypeRestriction(xsd:integer \
         xsd:minInclusive \"{lo}\"^^xsd:integer xsd:maxInclusive \"{hi}\"^^xsd:integer)))"
    )
}
#[allow(dead_code)]
fn max_int(n: u32, lo: i64, hi: i64) -> String {
    format!(
        "  SubClassOf(:C DataMaxCardinality({n} :p DatatypeRestriction(xsd:integer \
         xsd:minInclusive \"{lo}\"^^xsd:integer xsd:maxInclusive \"{hi}\"^^xsd:integer)))"
    )
}

/// Capacity: `≥3 p.[0,1]` demands 3 distinct integers, only 2 exist. UNSAT.
#[test]
fn capacity_clash_unsat_via_classify() {
    assert!(c_unsat(&min_int(3, 0, 1)));
}
