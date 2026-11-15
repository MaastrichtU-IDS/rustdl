//! #538: the operands of n-ary axioms are a SET (OWL 2 Structural Specification §2.1).
//!
//! A repeated operand used to be read as a list: `DifferentIndividuals(:a :a)` became
//! `a ≠ a` (a false inconsistency), `DisjointClasses(:C :C)` made `C` unsat,
//! `DisjointDataProperties(:p :p)` plus one `p` value made the ontology inconsistent, and
//! `DisjointObjectProperties(:r :r)` reported every sub-role of `r` disjoint from `r`.
//! Every case here pins the duplicate case at the set reading's answer next to a control
//! without the repeat that DOES give the positive, through the same call.
//!
//! The peers keep the repeats (Konclude and `HermiT` say `C` unsat for `DisjointClasses(:C
//! :C)`), so an oracle comparison shows these as rustdl-only misses by design.
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::justify::{Entailment, find_one_justification};

const NS: &str = "http://ex/#";

fn onto(body: &str) -> SetOntology<RcStr> {
    let src = format!(
        "Prefix(:=<http://ex/#>)\nPrefix(owl:=<http://www.w3.org/2002/07/owl#>)\n\
         Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)\nOntology(<http://ex/o>\n\
         Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C))\n\
         Declaration(Class(:D)) Declaration(Class(:E)) Declaration(Class(:F))\n\
         Declaration(Class(:Q))\n\
         Declaration(ObjectProperty(:r)) Declaration(ObjectProperty(:s))\n\
         Declaration(DataProperty(:p)) Declaration(DataProperty(:q))\n\
         Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))\n\
         {body}\n)\n"
    );
    read_ofn(&mut Cursor::new(src), ParserConfiguration::default())
        .expect("parse")
        .0
}

fn iri(x: &str) -> String {
    format!("{NS}{x}")
}

fn consistent(body: &str) -> bool {
    owl_dl_reasoner::is_consistent(&onto(body)).expect("is_consistent")
}

fn sat(body: &str, class: &str) -> bool {
    owl_dl_reasoner::is_class_satisfiable(&onto(body), &iri(class)).expect("is_class_satisfiable")
}

/// `classify`'s unsatisfiable classes (local names), sorted; `None` when inconsistent.
fn unsat(body: &str) -> Option<Vec<String>> {
    let h = owl_dl_reasoner::classify(&onto(body)).expect("classify");
    if h.stats().inconsistent {
        return None;
    }
    let mut u: Vec<String> = h
        .unsatisfiable_classes()
        .into_iter()
        .map(|c| c.trim_start_matches(NS).to_string())
        .collect();
    u.sort();
    Some(u)
}

fn names(v: &[&str]) -> Option<Vec<String>> {
    Some(v.iter().map(|s| (*s).to_string()).collect())
}

// ── DifferentIndividuals ────────────────────────────────────────────────────

#[test]
fn different_individuals_repeat_is_not_a_clash() {
    for body in [
        "DifferentIndividuals(:a :a) ClassAssertion(:C :a)",
        "DifferentIndividuals(:a :a :b) ClassAssertion(:C :a)",
        "DifferentIndividuals(:a :b :a :a) ClassAssertion(:C :a)",
    ] {
        assert!(consistent(body), "consistent: {body}");
        assert_eq!(unsat(body), names(&[]), "classify: {body}");
        assert!(sat(body, "C"), "sat C: {body}");
        let r = owl_dl_reasoner::realize(&onto(body)).expect("realize");
        assert_eq!(r.entailed_types(&iri("a")), [iri("C")], "realize: {body}");
    }
}

#[test]
fn different_individuals_keeps_the_distinct_pairs() {
    // Control: without the repeat, `a ≠ b` with `a = b` is a clash; with it, still.
    assert!(!consistent(
        "DifferentIndividuals(:a :b) SameIndividual(:a :b)"
    ));
    assert!(!consistent(
        "DifferentIndividuals(:a :a :b) SameIndividual(:a :b)"
    ));
    assert!(!consistent(
        "DifferentIndividuals(:b :a :b :a) SameIndividual(:a :b)"
    ));
    let d = owl_dl_reasoner::different_individuals(&onto("DifferentIndividuals(:a :a :b)"), None)
        .expect("different_individuals");
    assert_eq!(d.pairs(), [(iri("a"), iri("b"))]);
}

#[test]
fn same_individual_repeat_still_merges() {
    let s = owl_dl_reasoner::same_individuals(&onto("SameIndividual(:a :a :b)"), None)
        .expect("same_individuals");
    assert_eq!(s.groups(), [vec![iri("a"), iri("b")]]);
    assert!(consistent("SameIndividual(:a :a) ClassAssertion(:C :a)"));
    assert!(!consistent(
        "SameIndividual(:a :a :b) DifferentIndividuals(:a :b)"
    ));
}

// ── DisjointClasses ─────────────────────────────────────────────────────────

#[test]
fn disjoint_classes_repeat_says_nothing() {
    let body = "DisjointClasses(:C :C) SubClassOf(:D :C)";
    assert_eq!(unsat(body), names(&[]));
    assert!(sat(body, "C") && sat(body, "D"));
    assert!(consistent("DisjointClasses(:C :C) ClassAssertion(:C :a)"));
    assert!(consistent("DisjointClasses(owl:Thing owl:Thing)"));
    // An exact complex repeat.
    let complex = "DisjointClasses(ObjectSomeValuesFrom(:r :D) ObjectSomeValuesFrom(:r :D)) \
                   SubClassOf(:F ObjectSomeValuesFrom(:r :D))";
    assert!(sat(complex, "F"));
    // A structural repeat: the intersection's operands are a set too.
    let structural = "DisjointClasses(ObjectIntersectionOf(:A :B) ObjectIntersectionOf(:B :A)) \
                      SubClassOf(:E :A) SubClassOf(:E :B)";
    assert_eq!(unsat(structural), names(&[]));
}

#[test]
fn disjoint_classes_keeps_the_distinct_pairs() {
    // Control: the plain axiom.
    assert_eq!(
        unsat("DisjointClasses(:C :D) SubClassOf(:E :C) SubClassOf(:E :D)"),
        names(&["E"])
    );
    for body in [
        "DisjointClasses(:C :C :D) SubClassOf(:E :C) SubClassOf(:E :D)",
        "DisjointClasses(:C :D :C) SubClassOf(:E :C) SubClassOf(:E :D)",
        "DisjointClasses(:D :C :D :C) SubClassOf(:E :C) SubClassOf(:E :D)",
    ] {
        assert_eq!(unsat(body), names(&["E"]), "{body}");
        assert!(sat(body, "C") && !sat(body, "E"), "sat: {body}");
    }
}

/// Operands that denote the same class but are not structurally equal are NOT
/// collapsed, even where the concept pool interns them to one concept: the axiom then
/// says the class is empty.
#[test]
fn semantically_equal_operands_still_clash() {
    for body in [
        "DisjointClasses(:A ObjectIntersectionOf(:A owl:Thing))",
        "DisjointClasses(:A ObjectUnionOf(:A owl:Nothing))",
        "DisjointClasses(:A ObjectComplementOf(ObjectComplementOf(:A)))",
    ] {
        assert!(!sat(body, "A"), "{body}");
        assert_eq!(unsat(body), names(&["A"]), "{body}");
    }
    let assoc = "DisjointClasses(ObjectIntersectionOf(ObjectIntersectionOf(:A :B) :C) \
                 ObjectIntersectionOf(:A ObjectIntersectionOf(:B :C))) \
                 SubClassOf(:E :A) SubClassOf(:E :B) SubClassOf(:E :C)";
    assert!(!sat(assoc, "E"));
}

// ── DisjointUnion ───────────────────────────────────────────────────────────

#[test]
fn disjoint_union_repeat_is_an_equivalence() {
    let o = onto("DisjointUnion(:D :C :C)");
    let h = owl_dl_reasoner::classify(&o).expect("classify");
    assert!(h.unsatisfiable_classes().is_empty());
    assert!(
        h.equivalent_classes(&iri("D")).contains(&iri("C").as_str()),
        "D ≡ C"
    );
    let cert = owl_dl_reasoner::certified_unlowered(&o).expect("certified_unlowered");
    assert!(cert.classify.is_empty(), "{:?}", cert.classify);
    // The distinct members stay disjoint: control, then with the repeat.
    for body in [
        "DisjointUnion(:D :C :E) SubClassOf(:A :C) SubClassOf(:A :E)",
        "DisjointUnion(:D :C :C :E) SubClassOf(:A :C) SubClassOf(:A :E)",
        "DisjointUnion(:D :E :C :E) SubClassOf(:A :C) SubClassOf(:A :E)",
    ] {
        assert_eq!(unsat(body), names(&["A"]), "{body}");
    }
    // A built-in union class keeps #427's ordinary twin; no false positive either way.
    assert!(consistent(
        "DisjointUnion(owl:Thing :C :C) ClassAssertion(:C :a)"
    ));
    assert!(consistent("DisjointUnion(owl:Nothing :C :C)"));
}

// ── Object and data property disjointness ───────────────────────────────────

#[test]
fn disjoint_object_properties_repeat_says_nothing() {
    let body = "DisjointObjectProperties(:r :r) SubObjectPropertyOf(:s :r) \
                SubClassOf(:C ObjectSomeValuesFrom(:r :Q))";
    let o = onto(body);
    assert!(
        owl_dl_reasoner::disjoint_object_properties(&o)
            .unwrap()
            .is_empty()
    );
    let gaps = owl_dl_reasoner::property_structural_gaps(&o).unwrap();
    assert!(
        !gaps
            .object_disjoint()
            .iter()
            .any(|g| g == "object-empty-property"),
        "{gaps:?}"
    );
    assert!(sat(body, "C"));
    assert!(consistent(
        "DisjointObjectProperties(:r :r) ObjectPropertyAssertion(:r :a :b)"
    ));
    let inv = "DisjointObjectProperties(ObjectInverseOf(:r) ObjectInverseOf(:r)) \
               SubClassOf(:C ObjectSomeValuesFrom(:r :Q))";
    assert!(sat(inv, "C"));
    // Controls: the plain axiom reports its pair and empties a sub-role of both.
    let o = onto("DisjointObjectProperties(:r :s)");
    assert_eq!(
        owl_dl_reasoner::disjoint_object_properties(&o).unwrap(),
        [(iri("r"), iri("s"))]
    );
    let o = onto("DisjointObjectProperties(:r :r :s)");
    assert_eq!(
        owl_dl_reasoner::disjoint_object_properties(&o).unwrap(),
        [(iri("r"), iri("s"))]
    );
    assert!(!sat(
        "DisjointObjectProperties(:r :s) SubObjectPropertyOf(:r :s) \
         SubClassOf(:C ObjectSomeValuesFrom(:r :Q))",
        "C"
    ));
    // `r` and `r⁻` are different operands.
    assert!(!consistent(
        "DisjointObjectProperties(:r ObjectInverseOf(:r)) ObjectPropertyAssertion(:r :a :b) \
         ObjectPropertyAssertion(:r :b :a)"
    ));
}

#[test]
fn disjoint_data_properties_repeat_says_nothing() {
    // The data-axiom pass emitted `⊤ ⊑ ⊥` for `(p, p)` sharing a value.
    assert!(consistent(
        "DisjointDataProperties(:p :p) DataPropertyAssertion(:p :a \"1\"^^xsd:integer)"
    ));
    assert!(!consistent(
        "DisjointDataProperties(:p :q) DataPropertyAssertion(:p :a \"1\"^^xsd:integer) \
         DataPropertyAssertion(:q :a \"1\"^^xsd:integer)"
    ));
    assert!(!consistent(
        "DisjointDataProperties(:p :p :q) DataPropertyAssertion(:p :a \"1\"^^xsd:integer) \
         DataPropertyAssertion(:q :a \"1\"^^xsd:integer)"
    ));
    let o = onto("DisjointDataProperties(:p :p) SubDataPropertyOf(:q :p)");
    assert!(
        owl_dl_reasoner::disjoint_data_properties(&o)
            .unwrap()
            .is_empty()
    );
    let gaps = owl_dl_reasoner::property_structural_gaps(&o).unwrap();
    assert!(
        !gaps
            .data_disjoint()
            .iter()
            .any(|g| g == "data-empty-property"),
        "{gaps:?}"
    );
    let o = onto("DisjointDataProperties(:p :q)");
    assert_eq!(
        owl_dl_reasoner::disjoint_data_properties(&o).unwrap(),
        [(iri("p"), iri("q"))]
    );
}

// ── justify, the signature, the counter ─────────────────────────────────────

#[test]
fn justify_queries_keep_their_pairwise_reading() {
    let o = onto("ClassAssertion(:C :a)");
    let same = Entailment::SameIndividual {
        a: iri("a"),
        b: iri("a"),
    };
    let j = find_one_justification(&o, &same)
        .unwrap()
        .expect("a = a is entailed");
    assert!(j.axioms.is_empty(), "{:?}", j.axioms);
    let different = Entailment::DifferentIndividuals {
        a: iri("a"),
        b: iri("a"),
    };
    assert!(
        find_one_justification(&o, &different).unwrap().is_none(),
        "a ≠ a"
    );
    // `disjoint C C` asks whether C is unsat.
    let o = onto("DisjointClasses(:C :C)");
    let q = Entailment::DisjointClasses {
        a: iri("C"),
        b: iri("C"),
    };
    assert!(find_one_justification(&o, &q).unwrap().is_none());
    let o = onto("DisjointClasses(:C :D) SubClassOf(:E :C) SubClassOf(:E :D)");
    let q = Entailment::DisjointClasses {
        a: iri("E"),
        b: iri("E"),
    };
    assert!(find_one_justification(&o, &q).unwrap().is_some());
}

#[test]
fn entities_of_vacuous_axioms_stay_in_the_signature() {
    let src = "Prefix(:=<http://ex/#>)\nOntology(<http://ex/o>\n\
               DisjointClasses(:OnlyHere :OnlyHere) DifferentIndividuals(:solo :solo)\n)\n";
    let (o, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut Cursor::new(src), ParserConfiguration::default()).unwrap();
    let h = owl_dl_reasoner::classify(&o).unwrap();
    assert!(h.classes().contains(&iri("OnlyHere")), "{:?}", h.classes());
    let r = owl_dl_reasoner::realize(&o).unwrap();
    assert!(
        r.individuals().contains(&iri("solo")),
        "{:?}",
        r.individuals()
    );
}

#[test]
fn classify_reports_collapsed_operands() {
    let h = owl_dl_reasoner::classify(&onto(
        "DisjointClasses(:C :C) DifferentIndividuals(:a :a :b) DisjointUnion(:D :E :E)",
    ))
    .unwrap();
    let c = h.stats().collapsed_operands;
    assert_eq!((c.axioms, c.vacuous), (3, 1));
    assert!(h.stats().dropped.is_empty());
    let h = owl_dl_reasoner::classify(&onto("DisjointClasses(:C :D)")).unwrap();
    assert!(h.stats().collapsed_operands.is_empty());
}

/// `ObjectOneOf(:a :a)` is the singleton `{a}`, so it equates what it classifies with `a`
/// and the property-values surface flags that a value can travel along the equality, as
/// for `ObjectOneOf(:a)`.
#[test]
fn one_of_repeat_is_a_singleton_nominal() {
    let gap = |body: &str| {
        let g = owl_dl_reasoner::property_structural_gaps(&onto(body)).unwrap();
        g.data_values().iter().any(|k| k == "data-derived-equality")
    };
    let value = "ClassAssertion(:C :b) DataPropertyAssertion(:p :a \"1\"^^xsd:integer)";
    assert!(
        gap(&format!("{value} SubClassOf(:C ObjectOneOf(:a))")),
        "control"
    );
    assert!(gap(&format!("{value} SubClassOf(:C ObjectOneOf(:a :a))")));
    assert!(
        !gap(&format!("{value} SubClassOf(:C ObjectOneOf(:a :b))")),
        "two members"
    );
    assert!(!gap(value), "no nominal");
}

/// A one-operand axiom is no source for the property gaps: the gaps are those of the
/// ontology without it, while the two-operand control does raise the gap.
#[test]
fn one_operand_axioms_are_no_gap_source() {
    let gaps = |body: &str| {
        let g = owl_dl_reasoner::property_structural_gaps(&onto(body)).unwrap();
        format!("{g:?}")
    };
    let has = |body: &str, kind: &str| gaps(body).contains(kind);
    // Told class disjointness with two object-constrained (∀) properties:
    // object-class-constraint.
    let base =
        "SubClassOf(:C ObjectAllValuesFrom(:r :Q)) SubClassOf(:D ObjectAllValuesFrom(:s :Q))";
    assert!(
        has(
            &format!("{base} DisjointClasses(:C :D)"),
            "object-class-constraint"
        ),
        "control"
    );
    assert!(has(
        &format!("{base} DifferentIndividuals(:a :b)"),
        "object-class-constraint"
    ));
    for vacuous in [
        "DisjointClasses(:C :C)",
        "DisjointUnion(:E :C :C)",
        "DifferentIndividuals(:a :a)",
    ] {
        assert_eq!(gaps(&format!("{base} {vacuous}")), gaps(base), "{vacuous}");
    }
    // A single data value with an ABox: data-value-class-axiom.
    let value = "SubClassOf(:C DataSomeValuesFrom(:p DataOneOf(\"1\"^^xsd:integer)))";
    assert!(
        has(
            &format!("{value} SameIndividual(:a :b)"),
            "data-value-class-axiom"
        ),
        "control"
    );
    assert!(has(
        &format!("{value} DifferentIndividuals(:a :b)"),
        "data-value-class-axiom"
    ));
    for vacuous in ["SameIndividual(:a :a)", "DifferentIndividuals(:a :a)"] {
        assert_eq!(
            gaps(&format!("{value} {vacuous}")),
            gaps(value),
            "{vacuous}"
        );
    }
    // `DataOneOf("1" "1")` is the single value `{"1"}`.
    let one = |members: &str| {
        has(
            &format!(
                "ClassAssertion(:C :a) SubClassOf(:C DataSomeValuesFrom(:p DataOneOf({members})))"
            ),
            "data-value-class-axiom",
        )
    };
    assert!(one("\"1\"^^xsd:integer"), "control");
    assert!(one("\"1\"^^xsd:integer \"1\"^^xsd:integer"));
    assert!(!one("\"1\"^^xsd:integer \"2\"^^xsd:integer"), "two values");
    // `EquivalentClasses({a} {a})` equates nothing.
    let eq = "ClassAssertion(:C :b) DataPropertyAssertion(:p :a \"1\"^^xsd:integer)";
    assert!(has(
        &format!("{eq} EquivalentClasses(:C ObjectOneOf(:a))"),
        "data-derived-equality"
    ));
    assert_eq!(
        gaps(&format!(
            "{eq} EquivalentClasses(ObjectOneOf(:a) ObjectOneOf(:a))"
        )),
        gaps(eq)
    );
}
