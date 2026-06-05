//! Datatype completeness harness — measures which OWL 2 datatype
//! entailments rustdl currently DERIVES vs MISSES under the Phase D1
//! sound-under-approximation (data axioms silently dropped at convert
//! time).
//!
//! Each test loads a small synthetic fixture from `fixtures/datatype/`
//! that exercises ONE datatype feature in isolation, runs rustdl's
//! classify, and asserts the expected entailment. Tests are
//! `#[ignore]`d so they don't break the default `cargo test` run while
//! Tier B/C work is in flight; invoke explicitly via:
//!
//! ```sh
//! cargo test -p owl-dl-reasoner --test datatype_completeness \
//!     --release -- --ignored --nocapture
//! ```
//!
//! Per-test status (as of Phase D1 baseline measurement) is documented
//! in the test docstring. Tests that PASS today are guarding against
//! regressions; tests that FAIL document the D1 completeness gap +
//! become the TDD acceptance for Tier B/C implementation.
//!
//! Oracle generation: positive-entailment fixtures use ROBOT-with-HermiT
//! (`docker/robot/classify-oracle.sh fixture.ofn fixture-classified.owx`).
//! Unsat-producing fixtures (ROBOT errors on unsat) use direct
//! `is_unsatisfiable` assertions on the named classes — no OWX oracle
//! needed.
//!
//! See:
//! - `docs/phase2b-snapshot-results.md` for the project that closed the
//!   Horn-fragment headline (snapshot cache).
//! - The D1 commit (`e34aeb6`) that unlocked the 4 erroring fixtures
//!   via silent data-axiom drop.

#![allow(clippy::doc_markdown)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::classify_top_down_with_timeout;
use std::fs;
use std::io::Cursor;
use std::path::Path;
use std::time::Duration;

const FIXTURE_DIR: &str = "tests/fixtures/datatype";

fn classify_fixture(name: &str) -> owl_dl_reasoner::Classification {
    let path = Path::new(FIXTURE_DIR).join(format!("{name}.ofn"));
    let src = fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let mut reader = Cursor::new(src);
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut reader, ParserConfiguration::default()).expect("parse ofn");
    classify_top_down_with_timeout(&onto, Duration::from_millis(1_000)).expect("classify")
}

// ─────────────────────────────────────────────────────────────────────
// Positive-entailment tests (entailment expected; oracle as OWX).
// ─────────────────────────────────────────────────────────────────────

/// **SubDataPropertyOf transitivity**: `A ⊑ ∃specific.xsd:int` +
/// `∃general.xsd:int ⊑ B` + `SubDataPropertyOf(specific, general)`
/// ⇒ `A ⊑ B`. Requires data-property hierarchy propagation; D1 drops
/// SubDataPropertyOf so MISSED today.
#[test]
#[ignore = "Phase D4 (Tier B): PASSES via data-axiom preprocessing"]
fn sub_data_property_transitivity() {
    let c = classify_fixture("sub_data_property");
    let derived = c.is_subclass("http://t/A", "http://t/B");
    eprintln!(
        "sub_data_property_transitivity: A ⊑ B = {} (oracle: true)",
        derived
    );
    assert!(derived, "D1 MISSED: SubDataPropertyOf transitivity (A ⊑ B)");
}

/// **DataPropertyDomain inference**: `DataPropertyDomain(age, Person)` +
/// `Anything ⊑ ∃age.xsd:int` ⇒ `Anything ⊑ Person`. D1 drops
/// DataPropertyDomain.
#[test]
#[ignore = "Phase D4 (Tier B): PASSES via data-axiom preprocessing"]
fn data_property_domain_inference() {
    let c = classify_fixture("data_property_domain");
    let derived = c.is_subclass("http://t/Anything", "http://t/Person");
    eprintln!(
        "data_property_domain_inference: Anything ⊑ Person = {} (oracle: true)",
        derived
    );
    assert!(derived, "D1 MISSED: DataPropertyDomain (Anything ⊑ Person)");
}

/// **DatatypeDefinition + EquivalentClasses**: `Adult ≡ Person ⊓ ∃age.AgeAdult`
/// where `AgeAdult = xsd:integer[>=18]`. With no opposing definition,
/// this should derive `Adult ⊑ Person`. (Subsumption shouldn't need
/// datatype reasoning per se; tests parser/lowering of DatatypeDefinition.)
#[test]
#[ignore = "Phase D1 baseline: PASSES (Adult ⊑ Person is asserted directly; the EquivalentClasses axiom IS dropped by `ce_or_skip!` but the direct SubClassOf survives)"]
fn datatype_definition_subsumption() {
    let c = classify_fixture("datatype_definition");
    let derived = c.is_subclass("http://t/Adult", "http://t/Person");
    eprintln!(
        "datatype_definition_subsumption: Adult ⊑ Person = {} (oracle: true)",
        derived
    );
    assert!(derived, "Adult ⊑ Person should always hold (direct axiom)");
}

// ─────────────────────────────────────────────────────────────────────
// Unsat-entailment tests (HermiT/Konclude derive Bot; direct assertion).
// ─────────────────────────────────────────────────────────────────────

/// **FunctionalDataProperty + DataMinCardinality**: `Functional(age)`
/// (= `≤1 age`) + `HasTwoAges ⊑ ≥2 age` ⇒ `HasTwoAges ⊑ Bot`.
/// HermiT confirms. D1 drops both axioms so MISSED.
#[test]
#[ignore = "Phase D4 (Tier B): PASSES via data-axiom preprocessing"]
fn functional_data_property_unsat() {
    let c = classify_fixture("functional_data_property");
    let unsat = c.unsatisfiable_classes();
    let sub_bot = c.is_subclass(
        "http://t/HasTwoAges",
        "http://www.w3.org/2002/07/owl#Nothing",
    );
    eprintln!(
        "functional_data_property_unsat: unsat = {:?}, HasTwoAges ⊑ Nothing = {} (oracle: HasTwoAges + A unsat)",
        unsat, sub_bot
    );
    assert!(
        unsat.iter().any(|s| *s == "http://t/HasTwoAges") || sub_bot,
        "D1 MISSED: HasTwoAges should be unsat (Functional + ≥2)"
    );
}

/// **DataMin + DataMax disjointness**: `Big ⊑ ≥3 hasItem` +
/// `Small ⊑ ≤2 hasItem` ⇒ `Big ⊓ Small ⊑ Bot`. HermiT confirms
/// `Both ≡ Big ⊓ Small` is unsat. D1 drops data-cardinality so MISSED.
#[test]
#[ignore = "Phase D4 (Tier B): PASSES via data-axiom preprocessing"]
fn data_cardinality_disjointness() {
    let c = classify_fixture("data_cardinality_disjoint");
    let unsat = c.unsatisfiable_classes();
    eprintln!(
        "data_cardinality_disjointness: unsat = {:?} (oracle: Both)",
        unsat
    );
    assert!(
        unsat.iter().any(|s| *s == "http://t/Both"),
        "D1 MISSED: Both should be unsat (≥3 ⊓ ≤2 hasItem)"
    );
}

/// **Datatype facet conflict**: `Adult ⊑ ∃age.{integer ≥18}` +
/// `Child ⊑ ∃age.{integer <13}` + `Functional(age)` ⇒
/// `Adult ⊓ Child ⊑ Bot` (facet ranges disjoint at the value `[13,18)`).
/// HermiT confirms `Both ≡ Adult ⊓ Child` unsat. D1 drops everything
/// → MISSED.
#[test]
#[ignore = "Phase D5 (Tier C): PASSES via integer-range facet preprocessing"]
fn datatype_facet_disjointness() {
    let c = classify_fixture("datatype_facet");
    let unsat = c.unsatisfiable_classes();
    eprintln!(
        "datatype_facet_disjointness: unsat = {:?} (oracle: Both)",
        unsat
    );
    assert!(
        unsat.iter().any(|s| *s == "http://t/Both"),
        "D1 MISSED: Both should be unsat (age ≥18 ⊓ age <13, Functional)"
    );
}
