//! A `DataUnionOf` of `xsd:integer` INTERVALS in a universal / range position was
//! dropped (#42 item 1, residual).
//!
//! #42 item 1 closed the ENUMERATION half — `DataOneOf(a) ⊔ DataOneOf(b)` is
//! `DataOneOf(a,b)`, so flattening it is a logical identity. A union of INTERVALS has
//! no such single-range form: `[0,5] ⊔ [10,15]` is not an interval, and the integer
//! `DKey` bucket held one `IntegerRange` per key, so `data_range_dkey`'s parser chain
//! matched nothing and the whole axiom DROPPED. `∃p.{7} ⊓ ∀p.([0,5] ⊔ [10,15])` came
//! back satisfiable where Konclude derives it unsatisfiable — a silent MISS, pinned by
//! `data_union_of_enumerations.rs`'s scope guard until now.
//!
//! The fix widens that bucket from `IntegerRange` to `IntSet`, a union of ranges whose
//! `contains` / `disjoint` / `subset` are QUANTIFIED LIFTS of the scalar ops. A
//! one-component set therefore evaluates each op to exactly the old answer, and encodes
//! to exactly the old untagged IRI — so every key any pre-#42 ontology could produce is
//! unmoved by construction, and only genuinely multi-interval keys use the new `iset:`
//! form.
//!
//! ## Direction of risk
//!
//! Axioms that used to drop now CONVERT, and the new keys seed `DisjointClasses`. That
//! ADDS clashes, so the failure mode is a false POSITIVE. Hence the weight here sits on
//! the satisfiable controls, not the unsat ones — in particular the two GAP probes
//! (`6`, `9` outside `[0,5] ⊔ [10,15]`) paired with the two INCLUSIVE BOUNDARIES (`5`,
//! `10` inside it), which together are what would catch an over-eager normalisation
//! that merged the components into `[0,15]`.
//!
//! ## Oracle
//!
//! Every probe in this file was adjudicated against Konclude v0.7.0-1138 (2026-09-05):
//! **12/12 agree**, all on real output (1105 bytes satisfiable / 1121 unsatisfiable —
//! not the ~896-byte `Thing`/`Nothing` stub it emits on input it cannot read). Konclude
//! *does* report the relation on the unsatisfiable probes, so its silence on the
//! satisfiable ones is a discriminating control rather than the under-reporting it is
//! documented to do elsewhere.

#![allow(clippy::unwrap_used)]
#![allow(clippy::doc_markdown)]

use owl_dl_reasoner::classify;

fn load(body: &str) -> owl_dl_reasoner::Classification {
    let src = format!(
        "Prefix(:=<http://ex.org/>)\n\
         Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)\n\
         Ontology(<http://ex.org/t>\n{body}\n)\n"
    );
    let mut cur = std::io::Cursor::new(src);
    let (onto, _): (
        horned_owl::ontology::set::SetOntology<horned_owl::model::RcStr>,
        _,
    ) = horned_owl::io::ofn::reader::read(&mut cur, horned_owl::io::ParserConfiguration::default())
        .expect("parse");
    classify(&onto).expect("classify")
}

/// `∃p.{v} ⊓ ∀p.range` — is `A` unsatisfiable?
fn a_is_unsat(v: i64, range: &str) -> bool {
    !load(&format!(
        "Declaration(Class(:A)) Declaration(DataProperty(:p))
         SubClassOf(:A DataSomeValuesFrom(:p DataOneOf(\"{v}\"^^xsd:integer)))
         SubClassOf(:A DataAllValuesFrom(:p {range}))"
    ))
    .unsatisfiable_classes()
    .is_empty()
}

fn interval(min: i64, max: i64) -> String {
    format!(
        "DatatypeRestriction(xsd:integer xsd:minInclusive \"{min}\"^^xsd:integer \
         xsd:maxInclusive \"{max}\"^^xsd:integer)"
    )
}

/// `[0,5] ⊔ [10,15]` — the shape the scope guard in `data_union_of_enumerations.rs`
/// pinned as a known miss.
fn gapped_union() -> String {
    format!("DataUnionOf({} {})", interval(0, 5), interval(10, 15))
}

/// THE FLIP. `7` lies in the gap between the two components, so `A` is unsatisfiable —
/// the verdict Konclude has always given and rustdl used to miss.
#[test]
fn a_value_in_the_gap_between_two_intervals_clashes() {
    assert!(
        a_is_unsat(7, &gapped_union()),
        "7 is outside both [0,5] and [10,15] — A is unsatisfiable, and Konclude agrees"
    );
}

/// The gap probes either side of the seam, and the value past the top. These are what
/// distinguish a real interval SET from a hull: under a normalisation that merged the
/// components into `[0,15]`, `6` and `9` would both come back satisfiable.
#[test]
fn the_gap_is_not_silently_closed_into_a_hull() {
    for v in [6, 9] {
        assert!(
            a_is_unsat(v, &gapped_union()),
            "{v} is in the gap between [0,5] and [10,15]; reporting A satisfiable would \
             mean the two components had been merged into the hull [0,15]"
        );
    }
    assert!(
        a_is_unsat(16, &gapped_union()),
        "16 is above both components"
    );
}

/// THE FP GUARDS, oracle-adjudicated. Every value genuinely inside the union must stay
/// satisfiable — including both INCLUSIVE boundaries, `5` (top of the first component)
/// and `10` (bottom of the second), which is where an off-by-one in the gap test would
/// show up. Konclude reports no unsatisfiable class for any of these.
///
/// This is what separates "the fix works" from "anything with a union now clashes".
#[test]
fn values_inside_either_interval_stay_satisfiable() {
    for v in [0, 3, 5, 10, 12, 15] {
        assert!(
            !a_is_unsat(v, &gapped_union()),
            "{v} IS in [0,5] ⊔ [10,15] — A is satisfiable, and Konclude agrees"
        );
    }
}

/// ADJACENT components merge, and that is sound only because the integers are DISCRETE:
/// `[0,5] ⊔ [6,10]` has no integer between `5` and `6`, so it IS `[0,10]`.
///
/// `6` must stay satisfiable (a merge that refused to join touching components would
/// still get this right, so the discriminating half is `11`, which must clash — if the
/// merge instead joined across a real gap, nothing here would fail but the gap probes
/// above would). **The dense datatypes have no successor and cannot do this**, which is
/// why they need their own reasoning rather than a copy of this type.
#[test]
fn touching_intervals_are_one_interval_over_the_integers() {
    let adj = format!("DataUnionOf({} {})", interval(0, 5), interval(6, 10));
    assert!(!a_is_unsat(6, &adj), "6 is in [0,5] ⊔ [6,10] = [0,10]");
    assert!(a_is_unsat(11, &adj), "11 is above [0,10]");
}

/// Unbounded ends survive the encoding: `(-∞,5] ⊔ [10,∞)` keys as `iset:*:5;10:*`, and
/// the `*` must round-trip on both sides. `7` is the only one of the three in the gap.
#[test]
fn unbounded_components_round_trip_through_the_encoding() {
    let unb = "DataUnionOf(\
       DatatypeRestriction(xsd:integer xsd:maxInclusive \"5\"^^xsd:integer) \
       DatatypeRestriction(xsd:integer xsd:minInclusive \"10\"^^xsd:integer))";
    assert!(!a_is_unsat(3, unb), "3 ≤ 5");
    assert!(a_is_unsat(7, unb), "7 is in the gap (5,10)");
    assert!(!a_is_unsat(20, unb), "20 ≥ 10");
}

/// A union MIXING an enumeration with an interval is now represented EXACTLY, not
/// dropped — because on a discrete value space `{1}` IS the interval `[1,1]`, so
/// `{1} ⊔ [10,15]` is an interval set like any other.
///
/// **The three assertions are what prove "exact" rather than "dropped" or "half".** If
/// the axiom still dropped, `7` would come back satisfiable. If only the interval half
/// survived, `1` would clash. If only the enumeration half survived, `12` would clash.
/// All three hold, and Konclude agrees on all three.
///
/// This supersedes the scope guard
/// `a_union_mixing_an_enumeration_and_an_interval_is_not_partially_flattened` in
/// `data_union_of_enumerations.rs`, which asserted the same satisfiable verdicts for the
/// opposite reason (nothing was representable, so everything dropped).
#[test]
fn a_mixed_enumeration_and_interval_union_is_represented_exactly() {
    let mixed = format!(
        "DataUnionOf(DataOneOf(\"1\"^^xsd:integer) {})",
        interval(10, 15)
    );
    assert!(!a_is_unsat(1, &mixed), "1 is the enumeration member");
    assert!(!a_is_unsat(12, &mixed), "12 is inside [10,15]");
    assert!(
        a_is_unsat(7, &mixed),
        "7 is in neither half — if this were satisfiable the axiom would still be dropping"
    );
}

/// ALL-OR-NOTHING, and this is the guard that makes it load-bearing rather than tidy.
///
/// `[10,15] ⊔ xsd:decimal` denotes every decimal, so `∃p.{7}` is SATISFIABLE — Konclude
/// agrees — and the correct behaviour is to represent none of it: the union has a member
/// no interval set can express, so the axiom drops (visibly, in `dropped`).
///
/// If `collect`'s `_ => false` arm were loosened to skip the unrepresentable member, it
/// would keep `[10,15]` alone — a strictly NARROWER range, which in a `∀` position
/// manufactures the clash `7 ∉ [10,15]` and reports `A` unsatisfiable. **A false
/// positive.**
///
/// The fixture is chosen so the sabotage is *distinguishable by verdict*. The obvious
/// candidate — union with a `xsd:string` enumeration — is NOT: there the discarded half
/// admits no integer, so both the correct code and the loosened code end up agreeing
/// with Konclude's `unsatisfiable`, for opposite reasons. The discarded member has to
/// WIDEN the integer range for the sabotage to be visible, which is why this one is a
/// bare `xsd:decimal` — and that is also the shape that actually occurs in the corpus
/// (`ore_ont_5964`'s only union is of bare datatypes).
#[test]
fn a_union_with_an_unrepresentable_member_drops_whole_rather_than_narrowing() {
    let widened = format!("DataUnionOf({} xsd:decimal)", interval(10, 15));
    let c = load(&format!(
        "Declaration(Class(:A)) Declaration(DataProperty(:p))
         SubClassOf(:A DataSomeValuesFrom(:p DataOneOf(\"7\"^^xsd:integer)))
         SubClassOf(:A DataAllValuesFrom(:p {widened}))"
    ));
    assert!(
        c.unsatisfiable_classes().is_empty(),
        "[10,15] ⊔ xsd:decimal admits 7, so A is satisfiable (Konclude agrees). Keeping \
         only the representable half would narrow the range to [10,15] and manufacture \
         a false positive."
    );
    assert!(
        !c.stats().dropped.is_empty(),
        "the drop must be REPORTED, not silent — that is the difference between this \
         known miss and the one #42 item 1 closed"
    );
}

/// SUBSUMPTION seeding, both directions — the third `IntSet` op, which none of the
/// clash probes above reach (they all go through `disjoint` / `contains`).
///
/// **The position is load-bearing and the obvious choice is VACUOUS.** In an
/// EXISTENTIAL position a `DataUnionOf` never becomes an interval-set key at all: it is
/// split into a class-level disjunction before `data_range_dkey` is consulted, so
/// `∃p.[0,5] ⊑ ∃p.([0,5] ⊔ [10,15])` holds by disjunction introduction and is reported
/// with this entire fix reverted — verified by sabotage, which is how the first version
/// of this test was caught testing nothing. A `∀` position is where the told
/// `DKey ⊑ DKey` edge that `IntSet::subset` seeds is actually consulted.
///
/// `∀` is monotone in its filler, so `[0,5] ⊆ [0,5] ⊔ [10,15]` gives `Part ⊑ Whole`. The
/// converse must NOT be reported: a union is not contained in one of its parts. That
/// negative half is the FP guard — relaxing `IntSet::subset`'s outer `∀` to an `∃` would
/// emit the backwards edge and manufacture an unsound subsumption.
#[test]
fn a_part_is_subsumed_by_the_union_but_not_the_other_way_round() {
    let c = load(&format!(
        "Declaration(Class(:Part)) Declaration(Class(:Whole)) Declaration(DataProperty(:p))
         EquivalentClasses(:Part DataAllValuesFrom(:p {}))
         EquivalentClasses(:Whole DataAllValuesFrom(:p {}))",
        interval(0, 5),
        gapped_union()
    ));
    assert!(
        c.is_subclass("http://ex.org/Part", "http://ex.org/Whole"),
        "[0,5] ⊆ [0,5] ⊔ [10,15] and ∀ is monotone in its filler, so Part ⊑ Whole"
    );
    assert!(
        !c.is_subclass("http://ex.org/Whole", "http://ex.org/Part"),
        "a union is NOT contained in one of its parts — Whole ⊑ Part would be an \
         unsound subsumption"
    );
}
