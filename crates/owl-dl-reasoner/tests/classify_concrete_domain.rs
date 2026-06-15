//! Canaries for classify-level concrete-domain VERIFY: a class
//! unsatisfiable only by an integer or string counting clash (`≥3 p.[0,1]`
//! capacity, `≥3 ⊓ ≤2` conflict, `≥3 p.{"a","b"}` string capacity) must
//! appear unsatisfiable via `classify` — not just via `is_class_satisfiable`.
//! Before this feature, classify trusted the wedge's `Sat` (the wedge has no
//! `card_sat`) and missed these.
//!
//! NEGATIVES-FIRST: the FP-critical direction is a satisfiable class wrongly
//! reported unsatisfiable. Every `assert!(!c_unsat(...))` is a genuinely
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

/// Conflict: `≥3 p.[0,100]` with `≤2 p.[0,100]`. UNSAT via classify.
#[test]
fn min_max_conflict_unsat_via_classify() {
    assert!(c_unsat(&format!(
        "{}\n{}",
        min_int(3, 0, 100),
        max_int(2, 0, 100)
    )));
}

/// Inheritance: `D` carries `≥3 p.[0,1]`, `C ⊑ D`. Both unsat via classify
/// (exercises the saturation-subsumer downward check in the probe).
#[test]
fn inherited_counting_clash_unsat_via_classify() {
    let src = format!(
        "{PFX}Ontology(<http://t/o>\n  \
         Declaration(Class(:C)) Declaration(Class(:D)) Declaration(DataProperty(:p))\n  \
         SubClassOf(:D DataMinCardinality(3 :p DatatypeRestriction(xsd:integer \
         xsd:minInclusive \"0\"^^xsd:integer xsd:maxInclusive \"1\"^^xsd:integer)))\n  \
         SubClassOf(:C :D)\n)\n"
    );
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut Cursor::new(src), ParserConfiguration::default()).expect("parse ofn");
    let unsat = classify(&onto).expect("classify");
    let unsat = unsat.unsatisfiable_classes();
    assert!(unsat.contains(&"http://t/D"), "D unsat; got {unsat:?}");
    assert!(
        unsat.contains(&"http://t/C"),
        "C (⊑ D) unsat; got {unsat:?}"
    );
}

// ─── FP GATE: satisfiable data nodes MUST stay satisfiable via classify ───

/// `∃p.[0,10]` (≥1, 11 ints). SAT.
#[test]
fn datasome_sat_via_classify() {
    assert!(!c_unsat(
        "  SubClassOf(:C DataSomeValuesFrom(:p DatatypeRestriction(xsd:integer \
         xsd:minInclusive \"0\"^^xsd:integer xsd:maxInclusive \"10\"^^xsd:integer)))"
    ));
}

/// Tight-but-feasible: `≥2 p.[0,1]` — exactly 2 ints. SAT.
#[test]
fn exactly_enough_sat_via_classify() {
    assert!(!c_unsat(&min_int(2, 0, 1)));
}

/// `≥2 p.[0,10]` with `≤5 p.[0,10]` — room to spare. SAT.
#[test]
fn min_under_max_sat_via_classify() {
    assert!(!c_unsat(&format!(
        "{}\n{}",
        min_int(2, 0, 10),
        max_int(5, 0, 10)
    )));
}

/// `≤1 p.[0,10]` alone — always feasible. SAT.
#[test]
fn datamax_alone_sat_via_classify() {
    assert!(!c_unsat(&max_int(1, 0, 10)));
}

// ─── STRING BUCKET (classify level) ───────────────────────────────────────

/// String capacity clash via classify: `≥3 p.{"a","b"}` — 2-element
/// enumeration, 3 demanded. UNSAT. (Previously asserted SAT when strings were
/// unhandled; now strings are wired into the concrete-domain solver.)
#[test]
fn string_capacity_clash_unsat_via_classify() {
    assert!(c_unsat(
        "  SubClassOf(:C DataMinCardinality(3 :p DataOneOf(\"a\" \"b\")))"
    ));
}

/// Exactly enough strings: `≥2 p.{"a","b"}` — 2 demanded, 2 available. SAT.
/// FP GUARD: must NOT be reported unsatisfiable.
#[test]
fn string_exactly_enough_sat_via_classify() {
    assert!(!c_unsat(
        "  SubClassOf(:C DataMinCardinality(2 :p DataOneOf(\"a\" \"b\")))"
    ));
}

/// String `∃p.{"a","b"}` (DataSomeValuesFrom). No cardinality constraint. SAT.
/// FP GUARD.
#[test]
fn string_datasome_sat_via_classify() {
    assert!(!c_unsat(
        "  SubClassOf(:C DataSomeValuesFrom(:p DataOneOf(\"a\" \"b\")))"
    ));
}

/// `≥1000 p.xsd:string` — bare string = Top = ∞ capacity. SAT.
/// FP GUARD: large demand over an infinite domain must never clash.
#[test]
fn string_top_large_demand_sat_via_classify() {
    assert!(!c_unsat(
        "  SubClassOf(:C DataMinCardinality(1000 :p xsd:string))"
    ));
}

/// D11b probe (spec test gate): `∃p.{5} ⊓ ∀p.[0,3]`, 5 ∉ [0,3] ⟹ C unsat.
/// This is a *membership* clash (DKey disjointness), NOT counting — the
/// spec predicts the WEDGE already catches it in classify, so
/// `data_counting_classes` stays counting-only. If this FAILS, widen the
/// predicate to include ∀-over-DKey classes (see the spec).
#[test]
fn forall_exists_membership_clash_unsat_via_classify() {
    assert!(c_unsat(
        "  SubClassOf(:C DataHasValue(:p \"5\"^^xsd:integer))\n  \
         SubClassOf(:C DataAllValuesFrom(:p DatatypeRestriction(xsd:integer \
         xsd:minInclusive \"0\"^^xsd:integer xsd:maxInclusive \"3\"^^xsd:integer)))"
    ));
}

/// Inheritance + feasible: `D` carries `≥2 p.[0,1]` (exactly 2 ints — SAT),
/// `C ⊑ D`. The override fires for both (D is counting-constrained, C
/// inherits via subsumers) but the main-tableau verify finds both SAT —
/// proving the inheritance trigger does not over-fire into an FP.
#[test]
fn inherited_feasible_counting_sat_via_classify() {
    let src = format!(
        "{PFX}Ontology(<http://t/o>\n  \
         Declaration(Class(:C)) Declaration(Class(:D)) Declaration(DataProperty(:p))\n  \
         SubClassOf(:D DataMinCardinality(2 :p DatatypeRestriction(xsd:integer \
         xsd:minInclusive \"0\"^^xsd:integer xsd:maxInclusive \"1\"^^xsd:integer)))\n  \
         SubClassOf(:C :D)\n)\n"
    );
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut Cursor::new(src), ParserConfiguration::default()).expect("parse ofn");
    let unsat = classify(&onto).expect("classify");
    let unsat = unsat.unsatisfiable_classes();
    assert!(
        !unsat.contains(&"http://t/D"),
        "D (≥2 over 2 ints) must be SAT; got {unsat:?}"
    );
    assert!(
        !unsat.contains(&"http://t/C"),
        "C (⊑ D) must be SAT; got {unsat:?}"
    );
}

// ─── DENSE BUCKET CANARIES (via classify) ─────────────────────────────────

/// Float CLASH: `≥2 p.{1.5}` — single inclusive point, capacity 1. UNSAT.
#[test]
fn float_point_clash_unsat_via_classify() {
    assert!(c_unsat(
        "  SubClassOf(:C DataMinCardinality(2 :p \
         DatatypeRestriction(xsd:float xsd:minInclusive \"1.5\"^^xsd:float \
         xsd:maxInclusive \"1.5\"^^xsd:float)))"
    ));
}

/// Float FP GUARD: `≥1 p.{1.5}` — 1 value fits. Must stay SAT.
#[test]
fn float_point_ge1_sat_via_classify() {
    assert!(!c_unsat(
        "  SubClassOf(:C DataMinCardinality(1 :p \
         DatatypeRestriction(xsd:float xsd:minInclusive \"1.5\"^^xsd:float \
         xsd:maxInclusive \"1.5\"^^xsd:float)))"
    ));
}

/// Float FP GUARD: `≥1000 p.[0.0,100.0]` — dense interval, ∞ capacity. SAT.
#[test]
fn float_interval_large_demand_sat_via_classify() {
    assert!(!c_unsat(
        "  SubClassOf(:C DataMinCardinality(1000 :p \
         DatatypeRestriction(xsd:float xsd:minInclusive \"0.0\"^^xsd:float \
         xsd:maxInclusive \"100.0\"^^xsd:float)))"
    ));
}

/// Decimal CLASH: `≥2 p.{1.5}` decimal point. UNSAT.
#[test]
fn decimal_point_clash_unsat_via_classify() {
    assert!(c_unsat(
        "  SubClassOf(:C DataMinCardinality(2 :p \
         DatatypeRestriction(xsd:decimal xsd:minInclusive \"1.5\"^^xsd:decimal \
         xsd:maxInclusive \"1.5\"^^xsd:decimal)))"
    ));
}

/// Decimal FP GUARD: `≥1 p.{1.5}` — 1 fits. SAT.
#[test]
fn decimal_point_ge1_sat_via_classify() {
    assert!(!c_unsat(
        "  SubClassOf(:C DataMinCardinality(1 :p \
         DatatypeRestriction(xsd:decimal xsd:minInclusive \"1.5\"^^xsd:decimal \
         xsd:maxInclusive \"1.5\"^^xsd:decimal)))"
    ));
}

/// Decimal FP GUARD: `≥1000 p.[0,100]` decimal — dense, ∞ capacity. SAT.
#[test]
fn decimal_interval_large_demand_sat_via_classify() {
    assert!(!c_unsat(
        "  SubClassOf(:C DataMinCardinality(1000 :p \
         DatatypeRestriction(xsd:decimal xsd:minInclusive \"0\"^^xsd:decimal \
         xsd:maxInclusive \"100\"^^xsd:decimal)))"
    ));
}

/// Date CLASH: `≥2 p.{2020-01-01}` single date point. UNSAT.
#[test]
fn date_point_clash_unsat_via_classify() {
    assert!(c_unsat(
        "  SubClassOf(:C DataMinCardinality(2 :p \
         DatatypeRestriction(xsd:date xsd:minInclusive \"2020-01-01\"^^xsd:date \
         xsd:maxInclusive \"2020-01-01\"^^xsd:date)))"
    ));
}

/// Date FP GUARD: `≥1 p.{2020-01-01}` — 1 fits. SAT.
#[test]
fn date_point_ge1_sat_via_classify() {
    assert!(!c_unsat(
        "  SubClassOf(:C DataMinCardinality(1 :p \
         DatatypeRestriction(xsd:date xsd:minInclusive \"2020-01-01\"^^xsd:date \
         xsd:maxInclusive \"2020-01-01\"^^xsd:date)))"
    ));
}

/// Date FP GUARD: `≥1000 p.[2020-01-01,2021-12-31]` — dense range, ∞. SAT.
#[test]
fn date_interval_large_demand_sat_via_classify() {
    assert!(!c_unsat(
        "  SubClassOf(:C DataMinCardinality(1000 :p \
         DatatypeRestriction(xsd:date xsd:minInclusive \"2020-01-01\"^^xsd:date \
         xsd:maxInclusive \"2021-12-31\"^^xsd:date)))"
    ));
}

/// DateTime CLASH: `≥2 p.{2020-01-01T00:00:00}` single datetime point. UNSAT.
#[test]
fn datetime_point_clash_unsat_via_classify() {
    assert!(c_unsat(
        "  SubClassOf(:C DataMinCardinality(2 :p \
         DatatypeRestriction(xsd:dateTime xsd:minInclusive \
         \"2020-01-01T00:00:00\"^^xsd:dateTime \
         xsd:maxInclusive \"2020-01-01T00:00:00\"^^xsd:dateTime)))"
    ));
}

/// DateTime FP GUARD: `≥1 p.{2020-01-01T00:00:00}` — 1 fits. SAT.
#[test]
fn datetime_point_ge1_sat_via_classify() {
    assert!(!c_unsat(
        "  SubClassOf(:C DataMinCardinality(1 :p \
         DatatypeRestriction(xsd:dateTime xsd:minInclusive \
         \"2020-01-01T00:00:00\"^^xsd:dateTime \
         xsd:maxInclusive \"2020-01-01T00:00:00\"^^xsd:dateTime)))"
    ));
}

/// DateTime FP GUARD: `≥1000 p.[2020-01-01T00:00:00,2021-01-01T00:00:00]` dense. SAT.
#[test]
fn datetime_interval_large_demand_sat_via_classify() {
    assert!(!c_unsat(
        "  SubClassOf(:C DataMinCardinality(1000 :p \
         DatatypeRestriction(xsd:dateTime xsd:minInclusive \
         \"2020-01-01T00:00:00\"^^xsd:dateTime \
         xsd:maxInclusive \"2021-01-01T00:00:00\"^^xsd:dateTime)))"
    ));
}

// ─── PHASE 2: COUNTING-PAIR SUBSUMPTION (is_subclass) ─────────────────────
//
// Env-mutation safety: RUSTDL_COUNTING_PAIR_VERIFY is process-global.
// `phase2_gate_off_restores_the_miss` sets it to "0"; any concurrent
// `phase2_*` test that asserts `is_subclass == true` would flip its result
// and fail.  PHASE2_ENV_MUTEX serializes all phase2 tests (including those
// that only read the default) so the gate-off window is mutually exclusive.

static PHASE2_ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn parse(src: &str) -> SetOntology<RcStr> {
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut Cursor::new(src), ParserConfiguration::default()).expect("parse ofn");
    onto
}

/// Helper: acquire PHASE2_ENV_MUTEX (poison-tolerant).
fn phase2_lock() -> std::sync::MutexGuard<'static, ()> {
    PHASE2_ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Phase 2 headline: `C ⊑ ≥5 p.int` entails `C ⊑ D` where `D ≡ ≥3 p.int`
/// (cardinality monotonicity `≥5 ⟹ ≥3`). The default classifier trusted the
/// wedge `NotSubsumed` and missed this; counting-pair verification routes it
/// to the main tableau's `concrete_domain_clash`.
#[test]
fn phase2_cardinality_monotonicity_subsumption_is_found() {
    let _lock = phase2_lock();
    let src = "Prefix(:=<http://t/>)\n\
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)\n\
Ontology(<http://t/x>\n\
  Declaration(Class(:C)) Declaration(Class(:D)) Declaration(DataProperty(:p))\n\
  SubClassOf(:C DataMinCardinality(5 :p xsd:integer))\n\
  EquivalentClasses(:D DataMinCardinality(3 :p xsd:integer))\n\
)\n";
    let result = classify(&parse(src)).expect("classify");
    assert!(
        result.is_subclass("http://t/C", "http://t/D"),
        "C ⊑ D must be found via counting-pair verification (≥5 ⟹ ≥3)"
    );
}

/// FP GUARD: `≥3` does NOT entail `≥5`, so `C ⊑ D` must NOT be reported.
#[test]
fn phase2_weaker_lower_bound_is_not_subsumed() {
    let _lock = phase2_lock();
    let src = "Prefix(:=<http://t/>)\n\
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)\n\
Ontology(<http://t/x>\n\
  Declaration(Class(:C)) Declaration(Class(:D)) Declaration(DataProperty(:p))\n\
  SubClassOf(:C DataMinCardinality(3 :p xsd:integer))\n\
  EquivalentClasses(:D DataMinCardinality(5 :p xsd:integer))\n\
)\n";
    let result = classify(&parse(src)).expect("classify");
    assert!(
        !result.is_subclass("http://t/C", "http://t/D"),
        "≥3 must NOT be reported ⊑ ≥5 (false subsumption = FP)"
    );
}

/// FP GUARD: different property ⇒ no subsumption.
#[test]
fn phase2_different_property_is_not_subsumed() {
    let _lock = phase2_lock();
    let src = "Prefix(:=<http://t/>)\n\
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)\n\
Ontology(<http://t/x>\n\
  Declaration(Class(:C)) Declaration(Class(:D))\n\
  Declaration(DataProperty(:p)) Declaration(DataProperty(:q))\n\
  SubClassOf(:C DataMinCardinality(5 :p xsd:integer))\n\
  EquivalentClasses(:D DataMinCardinality(3 :q xsd:integer))\n\
)\n";
    let result = classify(&parse(src)).expect("classify");
    assert!(
        !result.is_subclass("http://t/C", "http://t/D"),
        "≥5 p must NOT be reported ⊑ ≥3 q (different property)"
    );
}

/// Subsumer-inheritance: C ⊑ X, X ⊑ ≥5 p.int, D ≡ ≥3 p.int ⇒ C ⊑ D found
/// (exercises the `counting_relevant` subsumer expansion — C carries no
/// counting axiom directly, only via its subsumer X).
#[test]
fn phase2_inherited_counting_subsumption_is_found() {
    let _lock = phase2_lock();
    let src = "Prefix(:=<http://t/>)\n\
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)\n\
Ontology(<http://t/x>\n\
  Declaration(Class(:C)) Declaration(Class(:X)) Declaration(Class(:D))\n\
  Declaration(DataProperty(:p))\n\
  SubClassOf(:C :X)\n\
  SubClassOf(:X DataMinCardinality(5 :p xsd:integer))\n\
  EquivalentClasses(:D DataMinCardinality(3 :p xsd:integer))\n\
)\n";
    let result = classify(&parse(src)).expect("classify");
    assert!(
        result.is_subclass("http://t/C", "http://t/D"),
        "C ⊑ D must be found (C inherits ≥5 from X)"
    );
}

/// RAII guard: removes `RUSTDL_COUNTING_PAIR_VERIFY` on drop, even if
/// the test body panics.  Must be dropped while PHASE2_ENV_MUTEX is still
/// held (i.e., declared AFTER `_lock` so Rust drops it first).
struct RemoveEnvGuard(&'static str);
impl Drop for RemoveEnvGuard {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        // SAFETY: single-var cleanup; PHASE2_ENV_MUTEX is still held at
        // drop time because `_lock` is declared before `_guard` in the
        // test and therefore drops after it (Rust reverses declaration order).
        unsafe { std::env::remove_var(self.0) };
    }
}

/// Gate: with RUSTDL_COUNTING_PAIR_VERIFY=0 the headline miss returns
/// (verifies the gate disables cleanly).
#[test]
fn phase2_gate_off_restores_the_miss() {
    let _lock = phase2_lock();
    // `_guard` is declared AFTER `_lock` so it drops first (Rust reverses
    // declaration order), guaranteeing the var is removed while the mutex
    // is still held — even if `classify` panics.
    #[allow(unsafe_code)]
    unsafe {
        std::env::set_var("RUSTDL_COUNTING_PAIR_VERIFY", "0");
    }
    let _guard = RemoveEnvGuard("RUSTDL_COUNTING_PAIR_VERIFY");
    let src = "Prefix(:=<http://t/>)\n\
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)\n\
Ontology(<http://t/x>\n\
  Declaration(Class(:C)) Declaration(Class(:D)) Declaration(DataProperty(:p))\n\
  SubClassOf(:C DataMinCardinality(5 :p xsd:integer))\n\
  EquivalentClasses(:D DataMinCardinality(3 :p xsd:integer))\n\
)\n";
    let result = classify(&parse(src)).expect("classify");
    let found = result.is_subclass("http://t/C", "http://t/D");
    assert!(
        !found,
        "with the gate off, the wedge Sat is trusted and C⊑D is missed"
    );
}
