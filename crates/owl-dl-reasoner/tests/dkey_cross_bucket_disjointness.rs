//! Canaries for CROSS-BUCKET DKey disjointness (#86).
//!
//! `DataPropertyRange(p, D1)` plus `DataPropertyAssertion(p, a, "v"^^D2)` is
//! INCONSISTENT when `D1` and `D2` have disjoint value spaces. rustdl missed
//! every numeric instance of this: `seed_dkey_subsumptions` buckets DKeys by
//! datatype and seeds subsumption only WITHIN a bucket (correct — that is what
//! fixed the v0.4.6–v0.4.9 FP where float and double were folded together and
//! reported EQUIVALENT), and D11b seeded `DisjointClasses` only within a bucket
//! too. So cross-bucket DKeys were neither comparable nor disjoint.
//!
//! ## NEGATIVES-FIRST, because the risk direction is INVERTED here
//!
//! Emitting disjointness ADDS clashes, so a wrong "disjoint" is a false UNSAT —
//! a false POSITIVE — not a miss. This is the subsystem that already shipped an
//! FP for months (the float/double fold). The FP guards below are therefore the
//! load-bearing half of this file, not the bug tests:
//!
//! **`xsd:integer` is a SUBSET of `xsd:decimal`, NOT disjoint from it.** Konclude
//! confirms both directions consistent. A fix that says "different numeric bucket
//! ⇒ disjoint" turns the MISS into an FP, and `integer_value_in_decimal_range_*`
//! is what catches that.
//!
//! Every verdict here is adjudicated against Konclude AND HermiT; see
//! `docs/known-limitations/dkey-numeric-buckets-are-not-disjoint.md`.

#![allow(clippy::unwrap_used)]
#![allow(clippy::doc_markdown)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use std::io::Cursor;

/// `DataPropertyRange(p, range)` + `DataPropertyAssertion(p, a, "lit"^^lit_ty)`.
fn range_and_value(range: &str, lit: &str, lit_ty: &str) -> bool {
    let src = format!(
        "Prefix(:=<http://ex#>)\n\
         Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)\n\
         Ontology(\n\
         Declaration(DataProperty(:p))\n\
         Declaration(NamedIndividual(:a))\n\
         DataPropertyRange(:p xsd:{range})\n\
         DataPropertyAssertion(:p :a \"{lit}\"^^xsd:{lit_ty})\n\
         )"
    );
    let mut reader = Cursor::new(src);
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut reader, ParserConfiguration::default()).unwrap();
    owl_dl_reasoner::is_consistent(&onto).unwrap()
}

// ── FP GUARDS (the load-bearing half) ───────────────────────────────────────

/// `xsd:integer ⊂ xsd:decimal`. Konclude and HermiT both say CONSISTENT.
/// A "different numeric bucket ⇒ disjoint" fix fails here.
#[test]
fn integer_value_in_decimal_range_is_consistent() {
    assert!(
        range_and_value("decimal", "1", "integer"),
        "xsd:integer is a SUBSET of xsd:decimal — declaring these buckets \
         disjoint is a FALSE POSITIVE, not a fix"
    );
}

/// The other direction, with an INTEGRAL decimal value. Both oracles report
/// consistent.
///
/// ## Do not "complete the matrix" from the `1.5` case
///
/// `DataPropertyRange(p, xsd:integer)` + `"1.5"^^xsd:decimal` IS inconsistent
/// (Konclude and HermiT agree), and rustdl misses it. That is tempting to read as
/// "so integer and decimal are disjoint after all" — they are not. The direction
/// is VALUE-dependent, not datatype-dependent:
///
/// | range | value | oracles |
/// |---|---|---|
/// | `xsd:decimal` | `"1"^^xsd:integer` | consistent |
/// | `xsd:integer` | `"1"^^xsd:decimal` | consistent |
/// | `xsd:integer` | `"1.5"^^xsd:decimal` | **inconsistent** |
///
/// `xsd:integer ⊆ xsd:decimal`, so no bucket-disjointness rule can close the
/// `1.5` row — it needs VALUE MEMBERSHIP (`1.5 ∉ integer`), which is different
/// machinery. Adding `int × dec` to `seed_cross_bucket_disjoint` would fail THIS
/// test and `integer_value_in_decimal_range_is_consistent`, which is the point of
/// both.
#[test]
fn decimal_value_in_integer_range_is_consistent() {
    assert!(
        range_and_value("integer", "1", "decimal"),
        "xsd:decimal and xsd:integer overlap — not a disjointness clash"
    );
}

/// Same datatype on both sides must never clash, in every bucket.
#[test]
fn same_datatype_never_clashes() {
    for (ty, lit) in [
        ("double", "1.0"),
        ("float", "1.0"),
        ("decimal", "1.0"),
        ("integer", "1"),
        ("string", "x"),
    ] {
        assert!(
            range_and_value(ty, lit, ty),
            "xsd:{ty} against itself must stay consistent"
        );
    }
}

// ── THE BUG: disjoint value spaces must clash ───────────────────────────────

/// #86's own two-axiom reproducer. HermiT: `owl:Thing is not satisfiable`;
/// Konclude: `false`.
#[test]
fn float_value_in_double_range_is_inconsistent() {
    assert!(
        !range_and_value("double", "1.0", "float"),
        "xsd:float and xsd:double have disjoint value spaces (#86)"
    );
}

#[test]
fn double_value_in_float_range_is_inconsistent() {
    assert!(!range_and_value("float", "1.0", "double"));
}

#[test]
fn float_value_in_decimal_range_is_inconsistent() {
    assert!(!range_and_value("decimal", "1.0", "float"));
}

#[test]
fn float_value_in_integer_range_is_inconsistent() {
    assert!(!range_and_value("integer", "1", "float"));
}

#[test]
fn double_value_in_integer_range_is_inconsistent() {
    assert!(!range_and_value("integer", "1", "double"));
}

/// Cross-FAMILY already worked before #86; pinned so a bucket refactor cannot
/// silently lose it.
#[test]
fn integer_value_in_string_range_is_inconsistent() {
    assert!(!range_and_value("string", "1", "integer"));
}
