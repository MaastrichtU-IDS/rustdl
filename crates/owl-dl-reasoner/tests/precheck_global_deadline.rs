//! The classify inconsistency pre-check honours the global deadline (#162).
//!
//! The pre-check's two bounded routes (`ABox` saturation and the ABox-seeded
//! wedge) ran against their OWN adaptive budget, up to 12 s, whatever the caller's
//! `--global-timeout-ms` said. On `ore_ont_1043` at a 500 ms global budget the
//! pre-check alone took 12.9 s. They now run against the earlier of the two
//! deadlines, and are skipped outright once the global deadline has passed.
//!
//! Direction of risk: a skipped or cut route yields NO verdict, so this can only
//! lose a detection (reported as `consistency_undetermined`), never invent one.

#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use std::io::Cursor;
use std::time::Duration;

fn onto(body: &str) -> SetOntology<RcStr> {
    let src = format!(
        "Prefix(:=<http://ex#>)\n\
         Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)\n\
         Ontology(\n{body}\n)"
    );
    read_ofn(&mut Cursor::new(src), ParserConfiguration::default())
        .expect("parse")
        .0
}

/// #89's reproducer: only the wedge route sees this inconsistency (the clash is
/// at a data successor, out of `abox_saturation`'s reach).
const FLOAT_IN_DOUBLE_RANGE: &str = r#"Declaration(DataProperty(:p))
    Declaration(NamedIndividual(:a))
    DataPropertyRange(:p xsd:double)
    DataPropertyAssertion(:p :a "1.0"^^xsd:float)"#;

#[test]
fn an_exhausted_global_budget_skips_the_bounded_routes_and_says_so() {
    let c = owl_dl_reasoner::classify_with_global_deadline(
        &onto(FLOAT_IN_DOUBLE_RANGE),
        Duration::ZERO,
    )
    .unwrap();
    // Before #162 the wedge ran out its own budget and found the clash here,
    // i.e. it ignored the caller's deadline.
    assert!(
        !c.stats().inconsistent,
        "with the global budget spent, the wedge route must not run"
    );
    assert!(
        c.stats().consistency_undetermined,
        "a skipped route must be reported, not read as consistent"
    );
}

#[test]
fn a_generous_global_budget_still_detects() {
    let c = owl_dl_reasoner::classify_with_global_deadline(
        &onto(FLOAT_IN_DOUBLE_RANGE),
        Duration::from_secs(60),
    )
    .unwrap();
    assert!(
        c.stats().inconsistent,
        "the wedge route must still run within budget"
    );
}
