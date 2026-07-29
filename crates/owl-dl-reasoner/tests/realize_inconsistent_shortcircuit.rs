//! `realize` / `materialize_inferred_class_assertions` must short-circuit on a
//! detectably-inconsistent ontology instead of grinding a `{a} ⊓ ¬C` tableau
//! probe per (individual, class).
//!
//! Motivation: on the (inconsistent) `family.ofn` torture fixture, `realize`
//! hung — its inconsistency is the deep multi-step kind that the `ABox`-saturation
//! pre-check catches (so `is_consistent` returns fast) but classify's own
//! pattern checks do not, so `realize` saw a large satisfiable-class set and ran
//! slow per-pair probes over a never-cheaply-clashing `ABox`. The sibling
//! `materialize_object_property_assertions` / `materialize_data_property_assertions`
//! already error on inconsistency via `saturate_abox_consistency`; `realize`
//! (and thus `materialize_inferred_class_assertions`) now does the same.
//!
//! Convention: an inconsistent ontology yields `Err(ReasonError::Inconsistent)`
//! (everything is vacuously entailed), matching the other `materialize_*` entry
//! points — not a silent empty/degenerate realization.

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::{ReasonError, realize};
use std::fs;
use std::io::Cursor;
use std::path::Path;

fn parse_ofn(body: &str) -> SetOntology<RcStr> {
    let src = format!(
        "Prefix(:=<http://t/>)\nPrefix(owl:=<http://www.w3.org/2002/07/owl#>)\nOntology(<http://t/x>\n{body}\n)\n"
    );
    let mut reader = Cursor::new(src);
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut reader, ParserConfiguration::default()).expect("parse ofn");
    onto
}

fn load(name: &str) -> SetOntology<RcStr> {
    let path = Path::new("tests/fixtures/regression").join(name);
    let src = fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {name}: {e}"));
    let mut reader = Cursor::new(src);
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut reader, ParserConfiguration::default()).expect("parse ofn");
    onto
}

#[test]
fn realize_errors_on_inconsistent_ontology() {
    // x : A, x : B, DisjointClasses(A, B) — inconsistent; the ABox-saturation
    // pre-check flags it, so realize must return Inconsistent, not a value.
    match realize(&load("realize_inconsistent.ofn")) {
        Err(ReasonError::Inconsistent) => {}
        other => panic!("expected Err(Inconsistent) on inconsistent ontology, got {other:?}"),
    }
}

#[test]
fn realize_still_succeeds_on_consistent_ontology() {
    // Guard: the short-circuit must not fire on a consistent ontology. The v3
    // nominal-cycle core is consistent and must still realize (a, b = Thing).
    let r = realize(&load("issue35_nominal_cycle_hang.ofn"))
        .expect("consistent ontology must realize, not error");
    for ind in r.individuals() {
        assert!(
            r.entailed_types(ind).iter().all(|t| !t.contains("hang3#")),
            "consistent realize regressed: {ind} got a defined type"
        );
    }
}

// ── Finding 4 (minor): `realize` on a `⊤ ⊑ ⊥` KB must error ────────────────
//
// Before the fix, `realize` on a `⊤ ⊑ ⊥` ontology silently returned an
// empty realization: `abox_saturation` misses the pure-TBox axiom (it only
// handles atomic-LHS SubClassOf), so the ABox pre-check was a no-op.
// With the new `has_top_subclass_bot` scan, `realize_internal` now returns
// `Err(ReasonError::Inconsistent)`, matching the convention of every other
// `materialize_*` entry point.

/// `realize` must return `Err(Inconsistent)` on a `⊤ ⊑ ⊥` KB.
#[test]
fn realize_errors_on_top_subclass_bot() {
    // SubClassOf(owl:Thing, owl:Nothing) — empty-domain, globally inconsistent.
    let onto = parse_ofn(
        "    Declaration(Class(:A))
    Declaration(NamedIndividual(:i))
    SubClassOf(owl:Thing owl:Nothing)",
    );
    match realize(&onto) {
        Err(ReasonError::Inconsistent) => {}
        other => panic!("expected Err(Inconsistent) on ⊤ ⊑ ⊥ ontology, got {other:?}"),
    }
}

/// Guard: `realize` must NOT error on a consistent ontology with an
/// individual — the `has_top_subclass_bot` scan must not over-fire.
#[test]
fn realize_consistent_with_individual_succeeds() {
    let onto = parse_ofn(
        "    Declaration(Class(:A))
    Declaration(Class(:B))
    Declaration(NamedIndividual(:i))
    SubClassOf(:A :B)
    ClassAssertion(:A :i)",
    );
    let r = realize(&onto).expect("consistent ontology with individual must realize, not error");
    let types = r.entailed_types("http://t/i");
    assert!(
        types.contains(&"http://t/A".to_string()),
        "individual i must be typed A"
    );
}
