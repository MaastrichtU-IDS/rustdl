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

/// SCOPE GUARD — a union of INTERVALS is deliberately NOT flattened, and this pins a
/// **known, oracle-confirmed sound MISS** rather than a correct answer.
///
/// `[0,5] ⊔ [10,15]` is not a single interval, and no range type here can represent a
/// disjoint set of them. A partial flatten would produce a strictly WEAKER range, which
/// in a `∀` position is unsound in the sufficient direction — so the axiom drops and `A`
/// comes back satisfiable.
///
/// **Konclude reports `A` UNSATISFIABLE here** (verified 2026-09-05, 1121 bytes of real
/// output — not the ~896-byte Thing/Nothing stub it emits on unreadable input), and it is
/// right: 7 is outside both intervals. So this test asserts a gap, not a verdict.
/// **FLIP it if an interval-set representation ever lands — do not delete it.**
#[test]
fn a_union_of_intervals_stays_a_sound_drop_and_is_a_known_miss() {
    let c = load(
        "Declaration(Class(:A)) Declaration(DataProperty(:p))
         SubClassOf(:A DataSomeValuesFrom(:p DataOneOf(\"7\"^^xsd:integer)))
         SubClassOf(:A DataAllValuesFrom(:p DataUnionOf(\
           DatatypeRestriction(xsd:integer xsd:minInclusive \"0\"^^xsd:integer xsd:maxInclusive \"5\"^^xsd:integer) \
           DatatypeRestriction(xsd:integer xsd:minInclusive \"10\"^^xsd:integer xsd:maxInclusive \"15\"^^xsd:integer))))",
    );
    assert!(
        c.unsatisfiable_classes().is_empty(),
        "a union of INTERVALS must stay a sound DROP — never a partial flatten. \
         Konclude derives unsat here, so this pins a known MISS: flip it when \
         interval-sets land, do not delete it."
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

/// THE SABOTAGE-DRIVEN GUARD. A union MIXING an enumeration with an interval must not
/// be partially flattened — and this is the case that makes the enumeration-only
/// restriction load-bearing rather than merely tidy.
///
/// `12 ∈ [10,15]`, so `A` is **satisfiable** (Konclude agrees, 1105 bytes of real
/// output). Correct behaviour: the union has a non-enumeration member, so nothing is
/// flattened and the axiom drops — a sound under-approximation.
///
/// If the flattener were loosened to accept any member, it would collect only the
/// enumeration's literals and yield `∀p.{1}`, **silently discarding the interval half**.
/// That is a strictly WEAKER range in a universal position, so `12 ∉ {1}` would
/// manufacture a clash and report `A` unsatisfiable — a **false positive**.
///
/// Found by sabotage: flipping `collect`'s `_ => false` arm to `_ => true` passed every
/// other test in this file, because a union of intervals alone flattens to an EMPTY
/// literal set and is rejected by the `lits.is_empty()` check anyway. Only a MIXED union
/// exposes it.
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
