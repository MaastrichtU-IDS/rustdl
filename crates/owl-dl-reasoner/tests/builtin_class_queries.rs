//! #427: the named-class queries resolve `owl:Thing` / `owl:Nothing` to the
//! built-in ⊤ / ⊥, whether or not the ontology declares them.
//!
//! On main they were refused (`class IRI not in ontology`) when undeclared, and
//! answered WRONGLY when declared: the declaration interned an axiom-free ordinary
//! class under the built-in's IRI, so `sat owl:Nothing` said "sat",
//! `A ⊑ owl:Thing` said "no", `owl:Thing` had no instances. Every cell below runs
//! on both forms, and the declared and undeclared answers must agree.
#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::justify::{Entailment, Verdict};
use owl_dl_reasoner::{ProveEntailmentResult, ReasonError};
use std::io::Cursor;
use std::time::Duration;

const THING: &str = "http://www.w3.org/2002/07/owl#Thing";
const NOTHING: &str = "http://www.w3.org/2002/07/owl#Nothing";
const A: &str = "http://ex/#A";
const B: &str = "http://ex/#B";
const T: &str = "http://ex/#T";
const U: &str = "http://ex/#U";
const IND: &str = "http://ex/#a";
const TYPO: &str = "http://ex/#Aa";

const DECLS: &str = "Declaration(Class(owl:Thing)) Declaration(Class(owl:Nothing))";

/// `A ⊑ B`, `U ⊑ ⊥`, `⊤ ⊑ T`, `a : A`. Pure EL.
const EL: &str = "Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:T))
  Declaration(Class(:U)) Declaration(Class(:C)) Declaration(Class(:D))
  Declaration(NamedIndividual(:a))
  SubClassOf(:A :B) SubClassOf(:U owl:Nothing) SubClassOf(owl:Thing :T)
  ClassAssertion(:A :a)";

/// [`EL`] plus a disjunction and a universal: out of EL and not Horn, so the
/// tableau and the non-Horn justification labelling are exercised.
const NON_EL: &str = "SubClassOf(:C ObjectUnionOf(:A :D))
  Declaration(ObjectProperty(:r)) SubClassOf(:D ObjectAllValuesFrom(:r :B))";

/// An `ABox` clash: `a : A`, `a : B`, `A ⊓ B ⊑ ⊥`.
const INCONSISTENT: &str = "Declaration(Class(:A)) Declaration(Class(:B))
  Declaration(NamedIndividual(:a))
  DisjointClasses(:A :B) ClassAssertion(:A :a) ClassAssertion(:B :a)";

fn onto(body: &str, declared: bool) -> SetOntology<RcStr> {
    let decls = if declared { DECLS } else { "" };
    let src = format!(
        "Prefix(:=<http://ex/#>)\nPrefix(owl:=<http://www.w3.org/2002/07/owl#>)\n\
         Ontology(<http://ex/o>\n{decls}\n{body}\n)"
    );
    read_ofn(&mut Cursor::new(src), ParserConfiguration::default())
        .expect("parse")
        .0
}

/// The consistent fixtures, each declared and undeclared, with a label.
fn consistent() -> Vec<(String, SetOntology<RcStr>)> {
    let mut out = Vec::new();
    for (name, body) in [("el", EL.to_owned()), ("non-el", format!("{EL}\n{NON_EL}"))] {
        for declared in [false, true] {
            out.push((format!("{name} declared={declared}"), onto(&body, declared)));
        }
    }
    out
}

fn sub(o: &SetOntology<RcStr>, sub: &str, sup: &str) -> bool {
    let (holds, stats) = owl_dl_reasoner::is_subclass_of_with_stats(o, sub, sup).unwrap();
    assert!(!stats.incomplete, "{sub} ⊑ {sup} gave up");
    holds
}

#[test]
fn satisfiability_of_the_built_ins() {
    for (label, o) in consistent() {
        let (sat_top, _) = owl_dl_reasoner::is_class_satisfiable_with_stats(&o, THING).unwrap();
        assert!(
            sat_top,
            "{label}: ⊤ is satisfiable on a consistent ontology"
        );
        let (sat_bot, _) = owl_dl_reasoner::is_class_satisfiable_with_stats(&o, NOTHING).unwrap();
        assert!(!sat_bot, "{label}: ⊥ is unsatisfiable");
        let budget = Duration::from_secs(30);
        assert_eq!(
            owl_dl_reasoner::is_class_satisfiable_with_timeout(&o, THING, budget).unwrap(),
            Some(true),
            "{label}"
        );
        assert_eq!(
            owl_dl_reasoner::is_class_satisfiable_with_timeout(&o, NOTHING, budget).unwrap(),
            Some(false),
            "{label}"
        );
    }
    for declared in [false, true] {
        let o = onto(INCONSISTENT, declared);
        let (sat_top, _) = owl_dl_reasoner::is_class_satisfiable_with_stats(&o, THING).unwrap();
        assert!(
            !sat_top,
            "declared={declared}: ⊤ is empty on an inconsistent ontology"
        );
    }
}

#[test]
fn subsumption_with_the_built_ins() {
    for (label, o) in consistent() {
        for (s, p, expected) in [
            (A, THING, true),
            (NOTHING, A, true),
            (THING, THING, true),
            (NOTHING, NOTHING, true),
            (NOTHING, THING, true),
            (U, NOTHING, true),
            (A, NOTHING, false),
            (THING, T, true),
            (THING, A, false),
            (THING, NOTHING, false),
        ] {
            assert_eq!(sub(&o, s, p), expected, "{label}: {s} ⊑ {p}");
        }
        // The saturation-only surface: a sound under-approximation, exact here.
        for (s, p, expected) in [
            (A, THING, true),
            (NOTHING, A, true),
            (U, NOTHING, true),
            (A, NOTHING, false),
            (THING, T, true),
            (THING, A, false),
            (THING, NOTHING, false),
        ] {
            assert_eq!(
                owl_dl_reasoner::is_subclass_of_saturation_only(&o, s, p).unwrap(),
                expected,
                "{label}: saturation-only {s} ⊑ {p}"
            );
        }
    }
    for declared in [false, true] {
        let o = onto(INCONSISTENT, declared);
        assert!(
            sub(&o, THING, NOTHING),
            "declared={declared}: ⊤ ⊑ ⊥ when inconsistent"
        );
        // #428: `A ⊑ ⊥` is `A`'s unsatisfiability, which errors on an inconsistent
        // KB; only `⊤ ⊑ ⊥`, the consistency check itself, answers.
        assert!(
            matches!(
                owl_dl_reasoner::is_subclass_of_with_stats(&o, A, NOTHING),
                Err(ReasonError::Inconsistent)
            ),
            "declared={declared}"
        );
        // Saturation-only does not see this ABox clash: "no" is its sound default.
        assert!(!owl_dl_reasoner::is_subclass_of_saturation_only(&o, THING, NOTHING).unwrap());
        // A TBox inconsistency the closure does witness (`⊤ ⊑ U ⊑ ⊥`).
        let tbox = onto(
            "Declaration(Class(:U)) SubClassOf(:U owl:Nothing) SubClassOf(owl:Thing :U)",
            declared,
        );
        assert!(
            owl_dl_reasoner::is_subclass_of_saturation_only(&tbox, THING, NOTHING).unwrap(),
            "declared={declared}: saturation-only ⊤ ⊑ ⊥ on ⊤ ⊑ U ⊑ ⊥"
        );
        assert!(sub(&tbox, THING, NOTHING), "declared={declared}");
    }
}

/// `⊤ ⊑ C` that only the tableau witnesses: `P ⊑ C` and `¬P ⊑ C` (excluded
/// middle). The closure cannot see it (saturation-only says "no", its sound
/// default); the full query goes through the `Q ≡ ⊤` probe to the tableau.
#[test]
fn top_subsumption_witnessed_only_by_the_tableau() {
    let body = "Declaration(Class(:P)) Declaration(Class(:C))
      SubClassOf(:P :C) SubClassOf(ObjectComplementOf(:P) :C)";
    let c = "http://ex/#C";
    for declared in [false, true] {
        let o = onto(body, declared);
        let (holds, stats) = owl_dl_reasoner::is_subclass_of_with_stats(&o, THING, c).unwrap();
        assert!(holds && !stats.incomplete, "declared={declared}");
        assert!(
            !stats.answered_by_saturation,
            "declared={declared}: the tableau decided"
        );
        assert!(!sub(&o, THING, "http://ex/#P"), "declared={declared}");
        assert!(
            !owl_dl_reasoner::is_subclass_of_saturation_only(&o, THING, c).unwrap(),
            "declared={declared}: the closure cannot witness excluded middle"
        );
    }
}

/// A typo beside a built-in is still an error, never a tautological "yes"
/// (#409), on every surface; and an unknown individual is named as such.
#[test]
fn unknown_iris_beside_a_built_in_still_error() {
    let o = onto(EL, true);
    let unknown = |r: Result<bool, ReasonError>| matches!(r, Err(ReasonError::UnknownClass(iri)) if iri == TYPO);
    for (s, p) in [
        (TYPO, THING),
        (NOTHING, TYPO),
        (THING, TYPO),
        (TYPO, NOTHING),
    ] {
        assert!(
            unknown(owl_dl_reasoner::is_subclass_of(&o, s, p)),
            "{s} ⊑ {p}"
        );
        assert!(
            unknown(owl_dl_reasoner::is_subclass_of_saturation_only(&o, s, p)),
            "saturation-only {s} ⊑ {p}"
        );
        assert!(
            matches!(
                owl_dl_reasoner::prove_entailment_rcstr_reporting(&o, s, p),
                Err(ReasonError::UnknownClass(iri)) if iri == TYPO
            ),
            "prove {s} ⊑ {p}"
        );
    }
    for class in [THING, NOTHING] {
        assert!(matches!(
            owl_dl_reasoner::is_instance_of_reporting(&o, class, "http://ex/#zz"),
            Err(ReasonError::UnknownIndividual(_))
        ));
    }
}

#[test]
fn instances_of_the_built_ins() {
    for (label, o) in consistent() {
        let (yes, flags) = owl_dl_reasoner::is_instance_of_reporting(&o, THING, IND).unwrap();
        assert!(yes && !flags.incomplete, "{label}: a : ⊤");
        let (no, flags) = owl_dl_reasoner::is_instance_of_reporting(&o, NOTHING, IND).unwrap();
        assert!(!no && !flags.incomplete, "{label}: a : ⊥ does not hold");
        assert!(owl_dl_reasoner::is_instance_of_saturation_only(&o, THING, IND).unwrap());
        assert!(!owl_dl_reasoner::is_instance_of_saturation_only(&o, NOTHING, IND).unwrap());

        let (members, flags) = owl_dl_reasoner::instances_of_reporting(&o, THING).unwrap();
        assert_eq!(members, vec![IND.to_owned()], "{label}");
        assert!(!flags.incomplete);
        let (members, _) = owl_dl_reasoner::instances_of_reporting(&o, NOTHING).unwrap();
        assert!(members.is_empty(), "{label}");
        assert_eq!(
            owl_dl_reasoner::instances_of_saturation_only(&o, THING).unwrap(),
            vec![IND.to_owned()],
            "{label}"
        );
        assert!(
            owl_dl_reasoner::instances_of_saturation_only(&o, NOTHING)
                .unwrap()
                .is_empty()
        );
    }
}

/// On an inconsistent ontology every instance query errors (#313 / #419), and ⊤ is
/// no exception: `a : ⊤` holds anyway, but the error is how callers learn the
/// ontology is broken. The built-ins take the ordinary instance path (on a fresh
/// `Q ≡ ⊤` / `Q ≡ ⊥`), so they get every guard it has; this fixture exercises the
/// #313 one (an `ABox` clash the pre-check sees), not #419's tableau-only case.
#[test]
fn instance_queries_on_the_built_ins_keep_the_inconsistency_error() {
    for declared in [false, true] {
        let o = onto(INCONSISTENT, declared);
        for class in [THING, NOTHING] {
            assert!(
                matches!(
                    owl_dl_reasoner::is_instance_of_reporting(&o, class, IND),
                    Err(ReasonError::Inconsistent)
                ),
                "declared={declared}: instance {class}"
            );
            assert!(
                matches!(
                    owl_dl_reasoner::instances_of_reporting(&o, class),
                    Err(ReasonError::Inconsistent)
                ),
                "declared={declared}: instances {class}"
            );
        }
    }
}

/// The declaration interns no class: the taxonomy is the undeclared one, and the
/// `Classification` accessors resolve the built-ins.
#[test]
fn classification_ignores_the_declaration_and_resolves_the_built_ins() {
    for body in [EL.to_owned(), format!("{EL}\n{NON_EL}")] {
        let plain = owl_dl_reasoner::classify(&onto(&body, false)).unwrap();
        let declared = owl_dl_reasoner::classify(&onto(&body, true)).unwrap();
        assert_eq!(plain.classes(), declared.classes());
        assert!(
            !declared
                .classes()
                .iter()
                .any(|c| c == THING || c == NOTHING)
        );
        for c in [&plain, &declared] {
            assert!(c.is_subclass(A, THING));
            assert!(c.is_subclass(NOTHING, A));
            assert!(c.is_subclass(U, NOTHING));
            assert!(!c.is_subclass(A, NOTHING));
            assert!(!c.is_subclass(THING, NOTHING));
            assert!(!c.is_subclass(TYPO, THING), "unknown IRIs read false");
            assert!(c.is_subclass(A, B));
        }
    }
    let inconsistent = owl_dl_reasoner::classify(&onto(INCONSISTENT, true)).unwrap();
    assert!(inconsistent.is_subclass(THING, NOTHING));
    assert!(inconsistent.is_subclass(A, NOTHING));
}

#[test]
fn prove_and_justify_the_built_in_forms() {
    for (label, o) in consistent() {
        // A tautology: the empty justification, minimal on every fragment.
        let (result, flags) =
            owl_dl_reasoner::prove_entailment_rcstr_reporting(&o, A, THING).unwrap();
        assert!(
            matches!(&result, ProveEntailmentResult::JustificationFallback(j)
                if j.axioms.is_empty() && j.minimal_guaranteed),
            "{label}: {result:?}"
        );
        assert!(!flags.incomplete);
        // `U ⊑ ⊥` has a saturator step proof.
        let (result, _) =
            owl_dl_reasoner::prove_entailment_rcstr_reporting(&o, U, NOTHING).unwrap();
        assert!(
            matches!(result, ProveEntailmentResult::SaturatorProof(_)),
            "{label}: {result:?}"
        );
        // `⊤ ⊑ T`: no step proof, but the asserted axiom justifies it.
        let (result, _) = owl_dl_reasoner::prove_entailment_rcstr_reporting(&o, THING, T).unwrap();
        assert!(
            matches!(&result, ProveEntailmentResult::JustificationFallback(j) if j.axioms.len() == 1),
            "{label}: {result:?}"
        );
        let (result, _) =
            owl_dl_reasoner::prove_entailment_rcstr_reporting(&o, A, NOTHING).unwrap();
        assert!(
            matches!(result, ProveEntailmentResult::NotEntailed),
            "{label}: {result:?}"
        );

        // justify: the tautology's justification is the empty set, marked minimal
        // even off the EL/Horn fragments.
        let q = Entailment::SubClassOf {
            sub: A.to_owned(),
            sup: THING.to_owned(),
        };
        assert_eq!(
            owl_dl_reasoner::justify::entails_reporting(&o, &q).unwrap(),
            Verdict::Entailed
        );
        let outcome = owl_dl_reasoner::justify::find_one_justification_reporting(&o, &q).unwrap();
        assert_eq!(outcome.status, Verdict::Entailed, "{label}");
        let j = &outcome.justifications[0];
        assert!(j.axioms.is_empty() && j.minimal_guaranteed, "{label}");
        // `U ⊑ owl:Nothing` is justified by `U ⊑ ⊥` alone.
        let q = Entailment::SubClassOf {
            sub: U.to_owned(),
            sup: NOTHING.to_owned(),
        };
        let outcome = owl_dl_reasoner::justify::find_one_justification_reporting(&o, &q).unwrap();
        assert_eq!(outcome.status, Verdict::Entailed, "{label}");
        assert_eq!(outcome.justifications[0].axioms.len(), 1, "{label}");
    }
    // `⊤ ⊑ ⊥` on an inconsistent ontology: the clash is its justification.
    let o = onto(INCONSISTENT, true);
    let q = Entailment::SubClassOf {
        sub: THING.to_owned(),
        sup: NOTHING.to_owned(),
    };
    let outcome = owl_dl_reasoner::justify::find_one_justification_reporting(&o, &q).unwrap();
    assert_eq!(outcome.status, Verdict::Entailed);
    assert_eq!(outcome.justifications[0].axioms.len(), 3);
}
