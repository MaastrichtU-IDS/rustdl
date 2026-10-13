//! #247: integer/decimal DKey relations beyond `RUSTDL_DKEY_INT_WITHIN_DECIMAL`'s
//! range ⊑ range edge (`RUSTDL_DKEY_INT_DECIMAL_CROSS`, default ON).
//!
//! The integer value space is INSIDE the decimal one: `12` IS the decimal `12`, and
//! the decimal `12.0` IS the integer `12`. Four shapes were silently missed (each
//! confirmed entailed by Konclude in the #245 review):
//! - an integer value under a `∀` over a decimal range it falls outside;
//! - an integer enumeration inside a decimal range;
//! - an integer inside a decimal enumeration;
//! - a whole-number decimal inside an integer range.
//!
//! **Direction of risk.** The subsumptions add positives, and the disjointness adds
//! clashes, so BOTH fail as false positives. A blanket integer/decimal disjointness
//! would make every in-range integer unsatisfiable. The negatives below carry the
//! weight, and each has a positive control so it cannot pass vacuously.
//!
//! Probes use the SUFFICIENT direction on the target (`∃p.R ⊑ W`), so a negative is a
//! real non-derivation rather than a vacuous one.

#![allow(clippy::unwrap_used, clippy::doc_markdown)]
#![allow(unsafe_code)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::classify_top_down_with_timeout;
use std::io::Cursor;
use std::sync::Mutex;
use std::time::Duration;

static ENV: Mutex<()> = Mutex::new(());

const PRE: &str = "Prefix(:=<http://ex.org/>)\n\
     Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)\n";

/// `A ⊑ have`, `want ⊑ W`. Returns `(A ⊑ W, A unsatisfiable)`.
fn probe(have: &str, want: &str) -> (bool, bool) {
    let _g = ENV
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    probe_unlocked(have, want)
}

fn probe_unlocked(have: &str, want: &str) -> (bool, bool) {
    let ofn = format!(
        "{PRE}Ontology(<http://ex.org/o>\n\
         Declaration(Class(:A)) Declaration(Class(:W)) Declaration(DataProperty(:dp))\n\
         SubClassOf(:A {have})\n\
         SubClassOf({want} :W)\n)\n"
    );
    let mut reader = Cursor::new(ofn);
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut reader, ParserConfiguration::default()).expect("parse ofn");
    let r = classify_top_down_with_timeout(&onto, Duration::from_secs(10)).expect("classify");
    let unsat = r.unsatisfiable_classes().contains(&"http://ex.org/A");
    (r.is_subclass("http://ex.org/A", "http://ex.org/W"), unsat)
}

fn some(dr: &str) -> String {
    format!("DataSomeValuesFrom(:dp {dr})")
}

fn all(dr: &str) -> String {
    format!("DataAllValuesFrom(:dp {dr})")
}

fn and(a: &str, b: &str) -> String {
    format!("ObjectIntersectionOf({a} {b})")
}

fn int(v: &str) -> String {
    format!("\"{v}\"^^xsd:integer")
}

fn dec(v: &str) -> String {
    format!("\"{v}\"^^xsd:decimal")
}

fn has(lit: &str) -> String {
    format!("DataHasValue(:dp {lit})")
}

fn one_of(lits: &[String]) -> String {
    format!("DataOneOf({})", lits.join(" "))
}

fn int_range(lo: &str, hi: &str) -> String {
    format!(
        "DatatypeRestriction(xsd:integer xsd:minInclusive {} xsd:maxInclusive {})",
        int(lo),
        int(hi)
    )
}

fn dec_range(facets: &str) -> String {
    format!("DatatypeRestriction(xsd:decimal {facets})")
}

// ---------------------------------------------------------------- disjointness

/// Issue item 1: `{12}` with every value a negative decimal is unsatisfiable.
#[test]
fn an_integer_value_outside_a_decimal_forall_is_unsatisfiable() {
    let neg = dec_range(&format!("xsd:maxExclusive {}", dec("0")));
    let (_, unsat) = probe(&and(&has(&int("12")), &all(&neg)), "xsd:decimal");
    assert!(unsat, "12 is no negative decimal: A ⊑ ⊥");
}

/// FP guard: the in-range control stays satisfiable, including on an inclusive
/// bound. A blanket integer/decimal disjointness would make both unsatisfiable.
#[test]
fn an_integer_value_inside_a_decimal_forall_stays_satisfiable() {
    let nonneg = dec_range(&format!("xsd:minInclusive {}", dec("0")));
    for v in ["12", "0"] {
        let (_, unsat) = probe(&and(&has(&int(v)), &all(&nonneg)), "xsd:decimal");
        assert!(!unsat, "{v} is a non-negative decimal: A satisfiable");
    }
}

/// FP guard: an integer RANGE with one integer inside an open decimal interval
/// overlaps it. `[1, 3]` holds `2`, which is in `(1.5, 2.5)`.
#[test]
fn an_integer_range_meeting_a_decimal_interval_stays_satisfiable() {
    let open = dec_range(&format!(
        "xsd:minExclusive {} xsd:maxExclusive {}",
        dec("1.5"),
        dec("2.5")
    ));
    let (_, unsat) = probe(
        &and(&some(&int_range("1", "3")), &all(&open)),
        "xsd:decimal",
    );
    assert!(!unsat, "2 ∈ [1, 3] ∩ (1.5, 2.5): A satisfiable");
    // Control: no integer of [1, 3] is in (3.5, 4.5).
    let far = dec_range(&format!(
        "xsd:minExclusive {} xsd:maxExclusive {}",
        dec("3.5"),
        dec("4.5")
    ));
    let (_, unsat) = probe(&and(&some(&int_range("1", "3")), &all(&far)), "xsd:decimal");
    assert!(unsat, "[1, 3] ∩ (3.5, 4.5) is empty: A ⊑ ⊥");
}

/// FP guard: a MULTI-interval integer key (a union, which only becomes one key in a
/// `∀` position) meets a decimal value if ANY interval does. `[1,2] ∪ [20,30]`
/// holds `25`; only a value in neither interval clashes.
#[test]
fn a_multi_interval_integer_key_meets_a_value_in_any_interval() {
    let union = format!(
        "DataUnionOf({} {})",
        int_range("1", "2"),
        int_range("20", "30")
    );
    let point = |v: &str| {
        dec_range(&format!(
            "xsd:minInclusive {} xsd:maxInclusive {}",
            dec(v),
            dec(v)
        ))
    };
    let (_, unsat) = probe(&and(&all(&union), &some(&point("25"))), "xsd:decimal");
    assert!(!unsat, "25 ∈ [20, 30]: A satisfiable");
    let (_, unsat) = probe(&and(&all(&union), &some(&point("10.5"))), "xsd:decimal");
    assert!(unsat, "10.5 is in neither interval: A ⊑ ⊥");
}

/// Integer value vs decimal ENUMERATION, both ways.
#[test]
fn an_integer_value_against_a_decimal_enumeration() {
    let e = one_of(&[dec("12.0"), dec("13.5")]);
    let (_, unsat) = probe(&and(&has(&int("12")), &all(&e)), "xsd:decimal");
    assert!(!unsat, "12 = 12.0 is a member: A satisfiable");
    let (_, unsat) = probe(&and(&has(&int("5")), &all(&e)), "xsd:decimal");
    assert!(unsat, "5 is not a member: A ⊑ ⊥");
}

/// Integer ENUMERATION vs decimal range and vs decimal enumeration.
#[test]
fn an_integer_enumeration_against_decimal_keys() {
    let ints = one_of(&[int("5"), int("7")]);
    let neg = dec_range(&format!("xsd:maxExclusive {}", dec("0")));
    let (_, unsat) = probe(&and(&some(&ints), &all(&neg)), "xsd:decimal");
    assert!(unsat, "{{5, 7}} ∩ (-∞, 0) is empty: A ⊑ ⊥");
    let pos = dec_range(&format!("xsd:minExclusive {}", dec("6")));
    let (_, unsat) = probe(&and(&some(&ints), &all(&pos)), "xsd:decimal");
    assert!(!unsat, "7 ∈ (6, ∞): A satisfiable");
    let (_, unsat) = probe(
        &and(&some(&ints), &all(&one_of(&[dec("7.0"), dec("8.5")]))),
        "xsd:decimal",
    );
    assert!(!unsat, "7 = 7.0 is shared: A satisfiable");
    let (_, unsat) = probe(
        &and(&some(&ints), &all(&one_of(&[dec("6.5"), dec("8.5")]))),
        "xsd:decimal",
    );
    assert!(unsat, "no shared value: A ⊑ ⊥");
}

/// Integer RANGE vs a decimal enumeration: only a whole-number member can meet it.
#[test]
fn an_integer_range_against_a_decimal_enumeration() {
    let (_, unsat) = probe(
        &and(
            &some(&int_range("1", "2")),
            &all(&one_of(&[dec("5.0"), dec("1.5")])),
        ),
        "xsd:decimal",
    );
    assert!(unsat, "neither 5.0 nor 1.5 is an integer in [1, 2]: A ⊑ ⊥");
    let (_, unsat) = probe(
        &and(
            &some(&int_range("1", "2")),
            &all(&one_of(&[dec("2.0"), dec("1.5")])),
        ),
        "xsd:decimal",
    );
    assert!(!unsat, "2.0 is the integer 2 ∈ [1, 2]: A satisfiable");
}

// ---------------------------------------------------------------- subsumption

/// Issue item 2a: an integer enumeration inside a decimal range.
#[test]
fn an_integer_enumeration_inside_a_decimal_range_is_subsumed() {
    let pos = dec_range(&format!("xsd:minExclusive {}", dec("0")));
    let (sub, _) = probe(&some(&one_of(&[int("12"), int("14")])), &some(&pos));
    assert!(sub, "{{12, 14}} ⊆ (0, ∞)");
    // FP guard: one member outside.
    let (sub, _) = probe(&some(&one_of(&[int("12"), int("-1")])), &some(&pos));
    assert!(!sub, "-1 ∉ (0, ∞)");
}

/// Issue item 2b: an integer inside a decimal enumeration.
#[test]
fn an_integer_value_inside_a_decimal_enumeration_is_subsumed() {
    let e = one_of(&[dec("12.0"), dec("13.5")]);
    assert!(
        probe(&has(&int("12")), &some(&e)).0,
        "12 = 12.0 ∈ {{12.0, 13.5}}"
    );
    assert!(!probe(&has(&int("13")), &some(&e)).0, "13 ∉ {{12.0, 13.5}}");
}

/// A finite integer range inside a decimal enumeration needs EVERY integer.
#[test]
fn a_finite_integer_range_inside_a_decimal_enumeration() {
    let full = one_of(&[dec("1.0"), dec("2.0"), dec("3")]);
    assert!(
        probe(&some(&int_range("1", "3")), &some(&full)).0,
        "[1, 3] ⊆ {{1, 2, 3}}"
    );
    let gap = one_of(&[dec("1.0"), dec("3.0"), dec("9")]);
    assert!(
        !probe(&some(&int_range("1", "3")), &some(&gap)).0,
        "2 ∉ {{1, 3, 9}}"
    );
}

/// Issue item 3: a whole-number decimal inside an integer key.
#[test]
fn a_whole_number_decimal_inside_an_integer_key_is_subsumed() {
    assert!(
        probe(&has(&dec("12.0")), &some("xsd:integer")).0,
        "12.0 is an integer"
    );
    assert!(
        probe(&has(&dec("12")), &some(&one_of(&[int("12"), int("14")]))).0,
        "12 ∈ {{12, 14}}"
    );
    // FP guards: fractional, and outside.
    assert!(
        !probe(&has(&dec("12.5")), &some("xsd:integer")).0,
        "12.5 is no integer"
    );
    assert!(
        !probe(&has(&dec("20")), &some(&int_range("0", "5"))).0,
        "20 ∉ [0, 5]"
    );
}

/// FP guard: a decimal range that is not a single point is dense, so it is never
/// inside an integer key, however wide.
#[test]
fn a_decimal_interval_is_never_inside_an_integer_key() {
    let interval = dec_range(&format!(
        "xsd:minInclusive {} xsd:maxInclusive {}",
        dec("1"),
        dec("3")
    ));
    assert!(!probe(&some(&interval), &some("xsd:integer")).0);
}

/// A decimal enumeration inside an integer key needs every member whole and inside.
#[test]
fn a_decimal_enumeration_inside_an_integer_key() {
    let target = some(&int_range("0", "5"));
    assert!(probe(&some(&one_of(&[dec("1.0"), dec("2")])), &target).0);
    assert!(
        !probe(&some(&one_of(&[dec("1.0"), dec("2.5")])), &target).0,
        "2.5"
    );
}

/// `RUSTDL_DKEY_INT_DECIMAL_CROSS=0` restores the old, unrelated buckets.
#[test]
fn the_flag_switches_the_cross_seeding_off() {
    let _g = ENV
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    // SAFETY: every env access in this binary holds `ENV`.
    unsafe { std::env::set_var("RUSTDL_DKEY_INT_DECIMAL_CROSS", "0") };
    let neg = dec_range(&format!("xsd:maxExclusive {}", dec("0")));
    let (_, unsat) = probe_unlocked(&and(&has(&int("12")), &all(&neg)), "xsd:decimal");
    let (sub, _) = probe_unlocked(&has(&dec("12.0")), &some("xsd:integer"));
    // SAFETY: as above.
    unsafe { std::env::remove_var("RUSTDL_DKEY_INT_DECIMAL_CROSS") };
    assert!(!unsat && !sub);
}
