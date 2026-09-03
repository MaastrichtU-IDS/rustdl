//! `ObjectHasSelf` was silently unenforced in the wedge (#90) — in BOTH
//! polarities, with and without an enclosing `∀`.
//!
//! ## The `∀` in the issue title was incidental
//!
//! The issue's control read "`Self` OUTSIDE a `∀` is fine", but that was checked
//! with `rustdl sat`, which passes. `classify` missed the two-axiom case too
//! (`self_outside_a_forall_is_also_detected` below): Konclude reports
//! `owl:Nothing ≡ :U`, and `classify` reported no unsatisfiable class. Worse than
//! the subsumption cases — `TRUST_SAT=0`, `CLASSIFY_VERIFY_REFUTATIONS=1` and every
//! counting-verify flag left it missed; only disabling the wedge recovered it.
//!
//! ## Root cause: TWO refusals, and each threw away the whole clause
//!
//! `eval_order` built a tree over a clause's role atoms and refused any body
//! whose atom targeted an already-bound variable. A clause it refused got NO
//! match plan at all, so it never fired and its constraint was silently
//! unenforced — a wedge `Sat` the wedge was not entitled to give.
//!
//! | shape | old refusal |
//! |---|---|
//! | `∀p.¬∃r.Self`, `∀p.(∃r.Self ⊔ ¬Z)`, and the no-`∀` case | `NotTree` — the self-loop `R(y,y)` / `R(X,X)` |
//! | `∀p.(¬∃r.Self ⊔ ¬Z)` | `Disconnected` — the naming clause was emitted on a successor variable no atom binds |
//!
//! Both had to be fixed, which is why anchoring the naming clause alone had been
//! measured as a no-op: on `X` the body becomes `R(X,X)`, which the *other*
//! refusal then discarded. A self-loop is not a tree edge but it is not
//! unsupported either — it is a FILTER, checked against already-bound nodes.
//!
//! ## Direction of risk
//!
//! Enforcing a previously-ignored body atom ADDS clashes, so the failure mode is
//! a false POSITIVE. `a_satisfiable_self_shape_stays_satisfiable` is the guard,
//! and it is oracle-adjudicated rather than assumed: Konclude reports no
//! unsatisfiable class for it. Without that control, "the fix works" would be
//! indistinguishable from "every `Self` body now clashes".

#![allow(clippy::unwrap_used)]
#![allow(clippy::doc_markdown)]

use owl_dl_reasoner::classify;

fn load(body: &str) -> owl_dl_reasoner::Classification {
    let src = format!("Prefix(:=<http://ex.org/>)\nOntology(<http://ex.org/t>\n{body}\n)\n");
    let mut cur = std::io::Cursor::new(src);
    let (onto, _): (
        horned_owl::ontology::set::SetOntology<horned_owl::model::RcStr>,
        _,
    ) = horned_owl::io::ofn::reader::read(&mut cur, horned_owl::io::ParserConfiguration::default())
        .expect("parse");
    classify(&onto).expect("classify")
}

fn entails(body: &str, sub: &str, sup: &str) -> bool {
    load(body).is_subclass(
        &format!("http://ex.org/{sub}"),
        &format!("http://ex.org/{sup}"),
    )
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

const DECLS: &str = "Declaration(Class(:S)) Declaration(Class(:T)) Declaration(Class(:Z))
     Declaration(ObjectProperty(:p)) Declaration(ObjectProperty(:r))";

/// `S` and `T` share a definition, so `T ⊑ S` holds for ANY filler. HermiT
/// confirms all three fillers below.
fn shared_definition(filler: &str) -> String {
    format!(
        "{DECLS}
         EquivalentClasses(:S ObjectAllValuesFrom(:p {filler}))
         SubClassOf(:T ObjectAllValuesFrom(:p {filler}))"
    )
}

/// BUG 1 — the single-literal negated `Self`. Refused as `NotTree`.
#[test]
fn negated_self_under_forall_entails_the_shared_definition() {
    assert!(
        entails(
            &shared_definition("ObjectComplementOf(ObjectHasSelf(:r))"),
            "T",
            "S"
        ),
        "T ⊑ S with filler ¬∃r.Self (#90)"
    );
}

/// BUG 2 — negated `Self` as a DISJUNCT. This one had a COMPLETE clause set
/// (0 deferred) and still failed, via the `Disconnected` refusal — which is what
/// proved the clausifier was never the whole story.
#[test]
fn negated_self_disjunct_under_forall_entails_the_shared_definition() {
    assert!(
        entails(
            &shared_definition(
                "ObjectUnionOf(ObjectComplementOf(ObjectHasSelf(:r)) ObjectComplementOf(:Z))"
            ),
            "T",
            "S"
        ),
        "T ⊑ S with filler (¬∃r.Self ⊔ ¬Z) (#90)"
    );
}

/// BUG 3 — POSITIVE `Self` as a disjunct. Never reaches the `Not` arm at all,
/// which is what rules out a negation-specific explanation.
#[test]
fn positive_self_disjunct_under_forall_entails_the_shared_definition() {
    assert!(
        entails(
            &shared_definition("ObjectUnionOf(ObjectHasSelf(:r) ObjectComplementOf(:Z))"),
            "T",
            "S"
        ),
        "T ⊑ S with filler (∃r.Self ⊔ ¬Z) (#90)"
    );
}

/// BUG 4 — and NO `∀` anywhere. Two axioms; Konclude reports
/// `EquivalentClasses(owl:Nothing, :U)`. The issue asserted this case was fine
/// because `rustdl sat` answered `unsat`; `classify` did not.
#[test]
fn self_outside_a_forall_is_also_detected() {
    assert_eq!(
        unsat(
            "Declaration(Class(:U)) Declaration(ObjectProperty(:r))
             SubClassOf(:U ObjectHasSelf(:r))
             SubClassOf(:U ObjectComplementOf(ObjectHasSelf(:r)))"
        ),
        vec!["U".to_string()],
        "U needs and forbids an r-self-loop, so it is unsatisfiable (#90)"
    );
}

/// CONTROL — the #78 shape, which always worked. A fix that broke this would be
/// touching disjunction handling rather than `Self`.
#[test]
fn the_78_shape_control_still_works() {
    assert!(
        entails(
            &shared_definition("ObjectUnionOf(ObjectComplementOf(:Z) ObjectComplementOf(:S))"),
            "T",
            "S"
        ),
        "the #78 filler must keep entailing T ⊑ S"
    );
}

/// THE FP GUARD, and the reason this fix is not just "more clashes". `C`'s
/// `p`-successor is a `B` with an `r`-successor that is a `D`, and `B`/`D` are
/// disjoint — so that successor is NOT the successor itself and there is no
/// `r`-self-loop. **Konclude reports no unsatisfiable class here.**
///
/// This is exactly what a fix that ignored the self-loop CONDITION (firing on any
/// `r`-successor) would break, and the pre-fix engine passed it only because it
/// ignored the clause altogether.
#[test]
fn a_satisfiable_self_shape_stays_satisfiable() {
    assert!(
        unsat(
            "Declaration(Class(:C)) Declaration(Class(:B)) Declaration(Class(:D))
             Declaration(ObjectProperty(:p)) Declaration(ObjectProperty(:r))
             SubClassOf(:C ObjectAllValuesFrom(:p ObjectComplementOf(ObjectHasSelf(:r))))
             SubClassOf(:C ObjectSomeValuesFrom(:p :B))
             SubClassOf(:B ObjectSomeValuesFrom(:r :D))
             DisjointClasses(:B :D)"
        )
        .is_empty(),
        "C is satisfiable (Konclude agrees): the r-successor is a D, not the node itself"
    );
}
