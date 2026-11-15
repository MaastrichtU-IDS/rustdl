//! #520: justify / repair / laconic on a query whose class or individual has no
//! `Declaration`. Subsets of the logical axioms lose an undeclared entity, and the
//! oracle's lookup used to abort the search with "IRI not in ontology". The search now
//! declares the query's looked-up entities (non-logical), so each subset treats a
//! missing one as the fresh symbol it is. Every fixture here has no declarations
//! unless it says so; the expected sets are exact, so a justification that is not
//! minimal, or that misses the fresh-symbol semantics, fails.
#![allow(clippy::unwrap_used)]

use std::collections::BTreeSet;
use std::io::Cursor;

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::{Component, RcStr};
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::ReasonError;
use owl_dl_reasoner::justify::{
    Entailment, PreparedJustifier, Verdict, find_all_justifications_reporting,
    find_one_justification_reporting, logical_axioms,
};
use owl_dl_reasoner::{find_all_laconic_justifications_reporting, find_repairs};

const P: &str = "http://ex/#";

fn onto(body: &str) -> SetOntology<RcStr> {
    let src = format!(
        "Prefix(:=<{P}>)\nPrefix(owl:=<http://www.w3.org/2002/07/owl#>)\n\
         Ontology(<http://ex/o>\n{body}\n)\n"
    );
    read_ofn(&mut Cursor::new(src), ParserConfiguration::default())
        .unwrap()
        .0
}

/// The logical axioms of `body`, as a set (the expected justification or repair).
fn axioms(body: &str) -> BTreeSet<Component<RcStr>> {
    logical_axioms(&onto(body)).1.into_iter().collect()
}

fn iri(local: &str) -> String {
    format!("{P}{local}")
}

fn instance(ind: &str, class: &str) -> Entailment {
    Entailment::InstanceOf {
        individual: iri(ind),
        class: iri(class),
    }
}

fn subclass(sub: &str, sup: &str) -> Entailment {
    Entailment::SubClassOf {
        sub: iri(sub),
        sup: iri(sup),
    }
}

/// Every justification surface agrees on the one expected justification: `find_one`,
/// `find_all`, the prepared variants and laconic (whose fragments of these one-conjunct
/// axioms are the axioms themselves).
#[track_caller]
fn assert_justification(o: &SetOntology<RcStr>, q: &Entailment, want: &BTreeSet<Component<RcStr>>) {
    let one = find_one_justification_reporting(o, q).unwrap();
    assert_eq!(one.status, Verdict::Entailed, "{q:?}");
    assert!(!one.incomplete, "{q:?}");
    let got: BTreeSet<_> = one.justifications[0].axioms.iter().cloned().collect();
    assert_eq!(&got, want, "find_one {q:?}");

    let all = find_all_justifications_reporting(o, q, 10).unwrap();
    assert_eq!(all.justifications.len(), 1, "find_all {q:?}");
    let got: BTreeSet<_> = all.justifications[0].axioms.iter().cloned().collect();
    assert_eq!(&got, want, "find_all {q:?}");

    let prepared = PreparedJustifier::prepare(o);
    let got: BTreeSet<_> = prepared.find_one_reporting(q).unwrap().justifications[0]
        .axioms
        .iter()
        .cloned()
        .collect();
    assert_eq!(&got, want, "prepared {q:?}");

    let lac = find_all_laconic_justifications_reporting(o, q, 10).unwrap();
    assert_eq!(lac.status, Verdict::Entailed, "laconic {q:?}");
    assert_eq!(lac.justifications.len(), 1, "laconic {q:?}");
}

/// The repairs are exactly the singletons of `want` (one justification, so each
/// axiom of it is a minimal repair), complete and verified.
#[track_caller]
fn assert_singleton_repairs(
    o: &SetOntology<RcStr>,
    q: &Entailment,
    want: &BTreeSet<Component<RcStr>>,
) {
    let r = find_repairs(o, q, 10).unwrap();
    assert!(r.entailed && r.complete && !r.incomplete, "{q:?}: {r:?}");
    let got: BTreeSet<BTreeSet<Component<RcStr>>> = r
        .repairs
        .iter()
        .map(|rep| rep.remove.iter().cloned().collect())
        .collect();
    let want: BTreeSet<BTreeSet<Component<RcStr>>> = want
        .iter()
        .map(|a| std::iter::once(a.clone()).collect())
        .collect();
    assert_eq!(got, want, "{q:?}");
}

const CHAIN: &str = "SubClassOf(:A :B) SubClassOf(:B :C) ClassAssertion(:A :a)";

/// C1: the #520 shape. `a` and every class are undeclared; `QuickXplain`'s subsets
/// without `a : A` used to fail the individual's lookup.
#[test]
fn an_undeclared_instance_query_is_justified_and_repaired() {
    let o = onto(CHAIN);
    let want = axioms(CHAIN);
    assert_justification(&o, &instance("a", "C"), &want);
    assert_singleton_repairs(&o, &instance("a", "C"), &want);

    // The all-declared control gives the same answers.
    let declared = onto(&format!(
        "Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C)) \
         Declaration(NamedIndividual(:a)) {CHAIN}"
    ));
    assert_justification(&declared, &instance("a", "C"), &want);
    assert_singleton_repairs(&declared, &instance("a", "C"), &want);
}

/// C2, C4: undeclared classes in `subclass` and `equivalent`.
#[test]
fn undeclared_subclass_and_equivalent_queries_are_justified() {
    let o = onto(CHAIN);
    assert_justification(
        &o,
        &subclass("A", "C"),
        &axioms("SubClassOf(:A :B) SubClassOf(:B :C)"),
    );

    let eq = "SubClassOf(:P :Q) SubClassOf(:Q :P) SubClassOf(:A :B)";
    let q = Entailment::EquivalentClasses {
        a: iri("P"),
        b: iri("Q"),
    };
    let one = find_one_justification_reporting(&onto(eq), &q).unwrap();
    assert_eq!(one.status, Verdict::Entailed);
    let got: BTreeSet<_> = one.justifications[0].axioms.iter().cloned().collect();
    assert_eq!(got, axioms("SubClassOf(:P :Q) SubClassOf(:Q :P)"));
}

/// C3: an undeclared unsatisfiable class.
#[test]
fn an_undeclared_unsatisfiable_class_is_justified() {
    let body = "SubClassOf(:V :U) SubClassOf(:U owl:Nothing) SubClassOf(:A :B)";
    let q = Entailment::Unsatisfiable { class: iri("V") };
    let want = axioms("SubClassOf(:V :U) SubClassOf(:U owl:Nothing)");
    assert_justification(&onto(body), &q, &want);
    assert_singleton_repairs(&onto(body), &q, &want);
}

/// C5: a fresh individual. `⊤ ⊑ Y` alone entails `a : Y`, so the justification does
/// not need `a`'s own assertion, and the only repair removes `⊤ ⊑ Y`. A subset without
/// `a : X` must treat `a` as fresh, not fail and not answer "no".
#[test]
fn a_fresh_individual_is_justified_by_top_alone() {
    let body = "SubClassOf(owl:Thing :Y) SubClassOf(:X :Z) ClassAssertion(:X :a)";
    let want = axioms("SubClassOf(owl:Thing :Y)");
    assert_justification(&onto(body), &instance("a", "Y"), &want);
    assert_singleton_repairs(&onto(body), &instance("a", "Y"), &want);
}

/// C6: an inconsistent ontology entails everything; the justification is the core,
/// in which the queried `b` (and its class) are fresh.
#[test]
fn an_inconsistency_core_justifies_an_undeclared_query() {
    let body = "ClassAssertion(:X :a) SubClassOf(:X owl:Nothing) ClassAssertion(:W :b) \
                SubClassOf(:Q :R)";
    let want = axioms("ClassAssertion(:X :a) SubClassOf(:X owl:Nothing)");
    let one = find_one_justification_reporting(&onto(body), &instance("b", "Q")).unwrap();
    assert_eq!(one.status, Verdict::Entailed);
    let got: BTreeSet<_> = one.justifications[0].axioms.iter().cloned().collect();
    assert_eq!(got, want);
}

/// C9: a fresh sup. `V` is unsatisfiable, so `V ⊑ C` holds without `C`'s axiom.
#[test]
fn a_fresh_superclass_is_justified_by_unsatisfiability() {
    let body = "SubClassOf(:V :U) SubClassOf(:U owl:Nothing) SubClassOf(:X :C)";
    let want = axioms("SubClassOf(:V :U) SubClassOf(:U owl:Nothing)");
    assert_justification(&onto(body), &subclass("V", "C"), &want);
}

/// C10: a fresh sub. `⊤ ⊑ C` alone entails `A ⊑ C`.
#[test]
fn a_fresh_subclass_is_justified_by_top_alone() {
    let body = "SubClassOf(owl:Thing :C) SubClassOf(:A :B)";
    let want = axioms("SubClassOf(owl:Thing :C)");
    assert_justification(&onto(body), &subclass("A", "C"), &want);
}

/// C11 (the stated behaviour change): `C` is outside the query's ⊥-module (only
/// `D ⊑ C` mentions it, and that axiom is ⊥-local for {a, C}) but in the ontology.
/// Main failed the module's lookup; the query now answers "not entailed".
#[test]
fn an_entity_outside_the_module_but_in_the_ontology_is_not_entailed() {
    let body = "SubClassOf(:D :C) ClassAssertion(:X :a)";
    let q = instance("a", "C");
    let one = find_one_justification_reporting(&onto(body), &q).unwrap();
    assert_eq!(one.status, Verdict::NotEntailed, "{one:?}");
    assert!(!one.incomplete && one.justifications.is_empty());
    let r = find_repairs(&onto(body), &q, 10).unwrap();
    assert!(!r.entailed && !r.incomplete, "{r:?}");
}

/// C7, C12, C13: an entity the ontology does not have, by kind, is main's error, in
/// main's order (an instance query's class first).
#[test]
fn an_entity_the_ontology_lacks_is_still_main_s_error() {
    let body = "SubClassOf(:A :B) ClassAssertion(:A :a) ObjectPropertyAssertion(:p :a :b)";
    let o = onto(body);
    let err = |q: &Entailment| find_one_justification_reporting(&o, q).unwrap_err();
    // C7: a typo'd class.
    assert!(
        matches!(err(&instance("a", "Typo")), ReasonError::UnknownClass(c) if c == iri("Typo")),
        "{:?}",
        err(&instance("a", "Typo"))
    );
    // C12: typed. A property IRI is not a class, a class IRI is not an individual.
    assert!(matches!(err(&instance("a", "p")), ReasonError::UnknownClass(c) if c == iri("p")));
    assert!(matches!(err(&instance("A", "B")), ReasonError::UnknownIndividual(i) if i == iri("A")));
    // C13: both missing: the class is reported first.
    assert!(
        matches!(err(&instance("nobody", "Nothing2")), ReasonError::UnknownClass(c) if c == iri("Nothing2"))
    );
    // A subclass query reports its sub first.
    assert!(matches!(err(&subclass("X1", "X2")), ReasonError::UnknownClass(c) if c == iri("X1")));
    // repair and the prepared path report the same error.
    assert!(matches!(
        find_repairs(&o, &instance("a", "Typo"), 10).unwrap_err(),
        ReasonError::UnknownClass(_)
    ));
}

/// C8: the kinds that never failed a lookup are unchanged.
#[test]
fn kinds_that_never_failed_are_unchanged() {
    let body = "ObjectPropertyAssertion(:p :a :b) SubObjectPropertyOf(:p :q) \
                SubClassOf(:U owl:Nothing) ClassAssertion(:A :a)";
    let o = onto(body);
    let q = Entailment::ObjectPropertyAssertion {
        source: iri("a"),
        prop: iri("q"),
        target: iri("b"),
    };
    let one = find_one_justification_reporting(&o, &q).unwrap();
    let got: BTreeSet<_> = one.justifications[0].axioms.iter().cloned().collect();
    assert_eq!(
        got,
        axioms("ObjectPropertyAssertion(:p :a :b) SubObjectPropertyOf(:p :q)")
    );
    let q = Entailment::DisjointClasses {
        a: iri("U"),
        b: iri("A"),
    };
    let one = find_one_justification_reporting(&o, &q).unwrap();
    let got: BTreeSet<_> = one.justifications[0].axioms.iter().cloned().collect();
    assert_eq!(got, axioms("SubClassOf(:U owl:Nothing)"));
}

/// C16: `prove`'s justification fallback (a non-saturator entailment) goes through the
/// same search. `⊤ ⊑ T` has no step proof; with nothing declared, main aborted at the
/// search's first subset probe.
#[test]
fn prove_s_justification_fallback_answers_an_undeclared_query() {
    let o = onto("SubClassOf(owl:Thing :T) SubClassOf(:A :B)");
    let (result, _) = owl_dl_reasoner::prove_entailment_rcstr_reporting(
        &o,
        "http://www.w3.org/2002/07/owl#Thing",
        &iri("T"),
    )
    .unwrap();
    assert!(
        matches!(&result, owl_dl_reasoner::ProveEntailmentResult::JustificationFallback(j) if j.axioms.len() == 1),
        "{result:?}"
    );
}

/// C8, continued: a sub-property and a data-value query on undeclared entities.
#[test]
fn more_kinds_that_never_failed_are_unchanged() {
    let body = "SubObjectPropertyOf(:p :q) SubObjectPropertyOf(:q :r) \
                SubDataPropertyOf(:d :e) DataPropertyAssertion(:d :a \"5\"^^<http://www.w3.org/2001/XMLSchema#integer>)";
    let o = onto(body);
    let q = Entailment::SubObjectProperty {
        sub: iri("p"),
        sup: iri("r"),
    };
    let one = find_one_justification_reporting(&o, &q).unwrap();
    let got: BTreeSet<_> = one.justifications[0].axioms.iter().cloned().collect();
    assert_eq!(
        got,
        axioms("SubObjectPropertyOf(:p :q) SubObjectPropertyOf(:q :r)")
    );
    let q = Entailment::DataPropertyValue {
        source: iri("a"),
        prop: iri("e"),
        value_lexical: "5".into(),
        value_datatype: "http://www.w3.org/2001/XMLSchema#integer".into(),
    };
    let one = find_one_justification_reporting(&o, &q).unwrap();
    let got: BTreeSet<_> = one.justifications[0].axioms.iter().cloned().collect();
    assert_eq!(
        got,
        axioms(
            "SubDataPropertyOf(:d :e) DataPropertyAssertion(:d :a \"5\"^^<http://www.w3.org/2001/XMLSchema#integer>)"
        )
    );
}
