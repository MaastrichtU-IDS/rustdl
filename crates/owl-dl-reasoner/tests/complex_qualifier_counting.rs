//! The wedge does no object cardinality counting over a COMPLEX qualifier (#91).
//!
//! `A ⊑ ≤1 r.(B ⊓ C)` together with `A ⊑ ≥2 r.(B ⊓ C)` makes `A` unsatisfiable.
//! Konclude and HermiT agree, `rustdl sat A` agrees — and `classify` reported
//! `unsatisfiable: []`.
//!
//! ## The third instance of one pattern
//!
//! classify's unsat probe trusts the wedge's `LabelOracle::Sat` unless
//! `needs_verify` fires, and that check already carved out two constructs the wedge
//! cannot count:
//!
//! | | construct |
//! |---|---|
//! | `data_counting_classes` | concrete-domain (DKey) cardinality |
//! | `nominal_counting_classes` (#49) | nominal counting — its comment reads "Same defect one construct over" |
//! | **this (#91)** | **object cardinality over a COMPLEX qualifier** |
//!
//! `cardinality_qualifier` Tseitin-names a complex filler, so the wedge counts a
//! synthetic name without relating it to the members and cannot see that the `≤`
//! and `≥` range over the SAME set. With a NAMED filler it can — which is what
//! isolates this to the qualifier shape rather than to cardinality, and why the
//! atomic control below must keep passing.
//!
//! ## Direction of risk
//!
//! Withdrawing trust only ever swaps a wedge `Sat` for the complete tableau path,
//! so this cannot invent an unsatisfiable class. Being wrong here costs wall time,
//! not correctness. The controls guard the cost side and the atomic path.

#![allow(clippy::unwrap_used)]
#![allow(clippy::doc_markdown)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use std::io::Cursor;

fn unsat_classes(body: &str) -> Vec<String> {
    let src = format!("Prefix(:=<http://ex#>)\nOntology(\n{body}\n)");
    let mut reader = Cursor::new(src);
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut reader, ParserConfiguration::default()).expect("parse");
    let c = owl_dl_reasoner::classify(&onto).expect("classify");
    let mut v: Vec<String> = c
        .unsatisfiable_classes()
        .into_iter()
        .map(|s| s.rsplit('#').next().unwrap_or(s).to_owned())
        .collect();
    v.sort();
    v
}

const HEAD: &str = "Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C))
     Declaration(ObjectProperty(:r))";

/// #91's own reproducer: `≤1` and `≥2` over the same complex filler.
#[test]
fn complex_qualifier_counting_clash_is_detected() {
    assert_eq!(
        unsat_classes(&format!(
            "{HEAD}
             SubClassOf(:A ObjectMaxCardinality(1 :r ObjectIntersectionOf(:B :C)))
             SubClassOf(:A ObjectMinCardinality(2 :r ObjectIntersectionOf(:B :C)))"
        )),
        vec!["A".to_string()],
        "≤1 and ≥2 over the SAME complex filler makes A unsatisfiable (#91)"
    );
}

/// THE CONTROL THAT ISOLATES THE DEFECT. With a NAMED filler the wedge counts
/// correctly and always did — this must keep passing, or the finding would have
/// been about cardinality rather than about the qualifier shape.
#[test]
fn the_atomic_filler_control_still_works() {
    assert_eq!(
        unsat_classes(&format!(
            "{HEAD}
             SubClassOf(:A ObjectMaxCardinality(1 :r :B))
             SubClassOf(:A ObjectMinCardinality(2 :r :B))"
        )),
        vec!["A".to_string()]
    );
}

// ── negative controls ───────────────────────────────────────────────────────

/// A complex qualifier with CONSISTENT bounds must not become unsatisfiable.
#[test]
fn a_satisfiable_complex_qualifier_stays_satisfiable() {
    assert!(
        unsat_classes(&format!(
            "{HEAD}
             SubClassOf(:A ObjectMaxCardinality(3 :r ObjectIntersectionOf(:B :C)))
             SubClassOf(:A ObjectMinCardinality(2 :r ObjectIntersectionOf(:B :C)))"
        ))
        .is_empty(),
        "≥2 and ≤3 are compatible — withdrawing wedge trust must not invent a clash"
    );
}

/// Bounds over DIFFERENT complex fillers are compatible: `≤1 r.(B⊓C)` and
/// `≥2 r.(B⊓D)` can both hold. This is the shape a careless fix would break by
/// treating any two complex qualifiers on one role as the same set.
#[test]
fn different_complex_qualifiers_do_not_clash() {
    assert!(
        unsat_classes(
            "Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C))
             Declaration(Class(:D)) Declaration(ObjectProperty(:r))
             SubClassOf(:A ObjectMaxCardinality(1 :r ObjectIntersectionOf(:B :C)))
             SubClassOf(:A ObjectMinCardinality(2 :r ObjectIntersectionOf(:B :D)))"
        )
        .is_empty(),
        "different fillers are different sets — no clash"
    );
}

/// An ontology with no qualified cardinality at all must be untouched: the
/// `complex_qualifier_counting_classes` set is empty and the clause short-circuits.
#[test]
fn ontologies_without_qualified_cardinality_are_inert() {
    assert!(
        unsat_classes("Declaration(Class(:A)) Declaration(Class(:B)) SubClassOf(:A :B)").is_empty()
    );
}
