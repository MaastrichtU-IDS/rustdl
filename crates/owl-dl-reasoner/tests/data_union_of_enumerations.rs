//! A `DataUnionOf` of enumerations in a UNIVERSAL / range position was dropped
//! (#42 item 1).
//!
//! `DataSomeValuesFrom(p, DataUnionOf(…))` has always worked — it lowers to a
//! class-level disjunction. A union in a `∀` or `DataPropertyRange` position cannot be
//! split that way, so `data_range_dkey`'s parser chain matched nothing and the whole
//! axiom was **dropped**: `∀p.({1} ⊔ {2})` alongside `∃p.{5}` reported `A` satisfiable
//! where Konclude and HermiT both report it unsatisfiable.
//!
//! The fix is a **logical identity** — `DataOneOf(a) ⊔ DataOneOf(b)` *is*
//! `DataOneOf(a, b)` — so it is sound and completeness-preserving by construction, not
//! by measurement. The flattened form goes back through the same per-datatype
//! `parse_*_oneof` chain, so datatype consistency is enforced where it already was
//! rather than re-checked here.
//!
//! ## Direction of risk
//!
//! This makes axioms CONVERT that previously dropped, so it adds constraints and the
//! failure mode is a false POSITIVE. `a_value_inside_the_union_stays_satisfiable` is the
//! guard, and it is oracle-adjudicated: Konclude reports no unsatisfiable class for it.
//! Without that control, "the unsat cases now report unsat" would be indistinguishable
//! from "everything with a union now clashes".

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

fn unsat(body: &str) -> Vec<String> {
    let mut v: Vec<String> = load(body)
        .unsatisfiable_classes()
        .into_iter()
        .map(|s| s.rsplit('/').next().unwrap_or(s).to_owned())
        .collect();
    v.sort();
    v
}

const DECLS: &str = "Declaration(Class(:A)) Declaration(DataProperty(:p))";
const UNION_1_2: &str = "DataUnionOf(DataOneOf(\"1\"^^xsd:integer) DataOneOf(\"2\"^^xsd:integer))";

/// BUG 1 — `∀` over a union. Konclude and HermiT both call `A` unsatisfiable.
#[test]
fn forall_over_a_union_of_enumerations_clashes_with_an_outside_value() {
    assert_eq!(
        unsat(&format!(
            "{DECLS}
             SubClassOf(:A DataSomeValuesFrom(:p DataOneOf(\"5\"^^xsd:integer)))
             SubClassOf(:A DataAllValuesFrom(:p {UNION_1_2}))"
        )),
        vec!["A".to_string()],
        "5 is outside {{1,2}}, so A is unsatisfiable (#42 item 1)"
    );
}

/// BUG 2 — `DataPropertyRange` over a union, the same shape one axiom form over.
#[test]
fn a_range_declared_as_a_union_of_enumerations_clashes_with_an_outside_value() {
    assert_eq!(
        unsat(&format!(
            "{DECLS}
             DataPropertyRange(:p {UNION_1_2})
             SubClassOf(:A DataSomeValuesFrom(:p DataOneOf(\"5\"^^xsd:integer)))"
        )),
        vec!["A".to_string()],
        "the range excludes 5, so A is unsatisfiable (#42 item 1)"
    );
}

/// BUG 3 — the NESTED COMPOSITE the issue names first,
/// `DataComplementOf(DataUnionOf(…))`. It works because flattening turns the inner
/// union into one enumeration, which the existing complement handling then accepts —
/// the nested case is unlocked by the normalisation rather than by its own code.
#[test]
fn a_complement_of_a_union_of_enumerations_is_handled() {
    assert_eq!(
        unsat(&format!(
            "{DECLS}
             SubClassOf(:A DataSomeValuesFrom(:p DataComplementOf({UNION_1_2})))
             SubClassOf(:A DataAllValuesFrom(:p DataOneOf(\"1\"^^xsd:integer \"2\"^^xsd:integer)))"
        )),
        vec!["A".to_string()],
        "A needs a value outside {{1,2}} and admits only values inside it (#42 item 1)"
    );
}

/// THE FP GUARD, oracle-adjudicated. A value INSIDE the union must stay satisfiable.
///
/// This is what separates "the fix works" from "anything with a union now clashes".
/// Konclude reports no unsatisfiable class here.
#[test]
fn a_value_inside_the_union_stays_satisfiable() {
    assert!(
        unsat(&format!(
            "{DECLS}
             SubClassOf(:A DataSomeValuesFrom(:p DataOneOf(\"2\"^^xsd:integer)))
             SubClassOf(:A DataAllValuesFrom(:p {UNION_1_2}))"
        ))
        .is_empty(),
        "2 IS in {{1,2}} — A is satisfiable, and Konclude agrees"
    );
}

/// SCOPE MARKER, **FLIPPED 2026-09-05** when the interval-set representation landed.
///
/// This test was written to pin a KNOWN, oracle-confirmed sound MISS: `[0,5] ⊔ [10,15]`
/// is not a single interval, the integer `DKey` bucket held one `IntegerRange` per key,
/// so the axiom dropped and `A` came back satisfiable while **Konclude derived it
/// UNSATISFIABLE** (verified 2026-09-05, 1121 bytes of real output — not the ~896-byte
/// `Thing`/`Nothing` stub it emits on unreadable input). Its own instruction was to flip
/// it rather than delete it when an interval-set representation arrived, so that the
/// closure of the gap would be recorded in the place that documented the gap.
///
/// It has arrived: the bucket now holds an `IntSet`, and this asserts the verdict rather
/// than the gap. `7` is outside both components, `A` is unsatisfiable, and rustdl and
/// Konclude agree. The full probe set — gap boundaries, inclusive endpoints, adjacency,
/// unbounded ends, mixed enumeration/interval unions — lives in
/// `data_union_of_intervals.rs`.
#[test]
fn a_union_of_intervals_now_clashes_with_a_value_in_the_gap() {
    let c = load(
        "Declaration(Class(:A)) Declaration(DataProperty(:p))
         SubClassOf(:A DataSomeValuesFrom(:p DataOneOf(\"7\"^^xsd:integer)))
         SubClassOf(:A DataAllValuesFrom(:p DataUnionOf(\
           DatatypeRestriction(xsd:integer xsd:minInclusive \"0\"^^xsd:integer xsd:maxInclusive \"5\"^^xsd:integer) \
           DatatypeRestriction(xsd:integer xsd:minInclusive \"10\"^^xsd:integer xsd:maxInclusive \"15\"^^xsd:integer))))",
    );
    assert_eq!(
        c.unsatisfiable_classes(),
        vec!["http://ex.org/A"],
        "7 is outside both [0,5] and [10,15] — the interval-set representation makes \
         this the clash Konclude always derived"
    );
}

/// A mixed-datatype union must not be flattened into a bucket that would compare
/// incomparable values — the trap the `xsd:float`/`xsd:double` FP came from. Enforced
/// by the per-datatype `parse_*_oneof` chain rather than re-checked in the flattener,
/// so this pins that the delegation actually holds.
#[test]
fn a_mixed_datatype_union_does_not_produce_a_clash() {
    assert!(
        unsat(
            "Declaration(Class(:A)) Declaration(DataProperty(:p))
             SubClassOf(:A DataSomeValuesFrom(:p DataOneOf(\"5\"^^xsd:integer)))
             SubClassOf(:A DataAllValuesFrom(:p DataUnionOf(
               DataOneOf(\"1\"^^xsd:integer) DataOneOf(\"a\"^^xsd:string))))"
        )
        .is_empty(),
        "a union spanning two datatypes must not be folded into one bucket"
    );
}

/// THE SABOTAGE-DRIVEN GUARD, **REFRAMED 2026-09-05**: still a false-positive guard,
/// but it now guards a different mechanism, because the interval-set representation
/// changed why it passes.
///
/// `12 ∈ [10,15]`, so `A` is **satisfiable** (Konclude agrees, 1105 bytes of real
/// output) and that verdict is what this asserts either way. What changed is the route:
///
/// - **Then:** the union had a non-enumeration member, so `flatten_union_of_oneofs`
///   declined and the whole axiom DROPPED — satisfiable because nothing constrained `p`.
/// - **Now:** on a discrete value space `{1}` IS `[1,1]`, so the union is an interval
///   set and is represented EXACTLY — satisfiable because `12` really is in it.
///
/// The original sabotage still holds: loosening `collect`'s `_ => false` arm would
/// collect only the enumeration's literals and yield `∀p.{1}`, silently discarding the
/// interval half — a strictly WEAKER range in a universal position, so `12 ∉ {1}` would
/// manufacture a clash. That sabotage was found only because a union of intervals ALONE
/// flattens to an empty literal set the `lits.is_empty()` check rejects anyway; only a
/// MIXED union exposes it.
///
/// **This test can no longer tell "represented exactly" from "dropped", so it is no
/// longer sufficient on its own** — `a_mixed_enumeration_and_interval_union_is_
/// represented_exactly` in `data_union_of_intervals.rs` adds the probe that
/// discriminates them (a value in NEITHER half, which must clash).
#[test]
fn a_union_mixing_an_enumeration_and_an_interval_is_not_partially_flattened() {
    assert!(
        unsat(
            "Declaration(Class(:A)) Declaration(DataProperty(:p))
             SubClassOf(:A DataSomeValuesFrom(:p DataOneOf(\"12\"^^xsd:integer)))
             SubClassOf(:A DataAllValuesFrom(:p DataUnionOf(DataOneOf(\"1\"^^xsd:integer) \
               DatatypeRestriction(xsd:integer xsd:minInclusive \"10\"^^xsd:integer xsd:maxInclusive \"15\"^^xsd:integer))))"
        )
        .is_empty(),
        "12 is in [10,15] so A is satisfiable (Konclude agrees) — a partial flatten to \
         `∀p.{{1}}` would discard the interval half and manufacture a false positive"
    );
}
