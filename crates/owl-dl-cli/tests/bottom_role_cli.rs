//! #539 on the CLI: `owl:bottomObjectProperty` / `owl:bottomDataProperty` are empty, and a
//! universal role told empty or negatively asserted is a contradiction. Before the fix
//! every engine read the bottom roles as ordinary roles: `C ⊑ ∃bottom.D` classified `C`
//! satisfiable with `completeness_guaranteed: true`, and `r ⊑ bottom`, `r(a, b)` read
//! consistent, so `realize` / `instance` gave a definite wrong answer.
//!
//! Negatives first. Every expectation follows the OWL 2 Direct Semantics and agrees with
//! `HermiT`. Konclude 0.7 agrees on the `TBox`, data, negative-control and
//! negated-universal cases, but calls every object-bottom `ABox` case and every told-empty
//! universal role consistent (an under-report; `sweeps/p539/canaries/oracle.out`).
#![allow(clippy::unwrap_used)]

use std::path::PathBuf;
use std::process::{Command, Output};

const HEAD: &str = "Prefix(:=<http://ex/#>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Prefix(rdfs:=<http://www.w3.org/2000/01/rdf-schema#>)
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)
Ontology(<http://ex/o>
Declaration(Class(:C)) Declaration(Class(:D)) Declaration(Class(:E))
Declaration(ObjectProperty(:r)) Declaration(ObjectProperty(:s)) Declaration(DataProperty(:p))
Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b)) Declaration(NamedIndividual(:c))
";

/// Writes one fixture to a per-test file (tests run in parallel).
struct Fixture(PathBuf);

impl Fixture {
    fn new(tag: &str, body: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "rustdl-bottom-role-{}-{tag}.ofn",
            std::process::id()
        ));
        std::fs::write(&path, format!("{HEAD}{body}\n)\n")).unwrap();
        Self(path)
    }

    fn path(&self) -> &str {
        self.0.to_str().unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_rustdl"))
        .env("RAYON_NUM_THREADS", "1")
        .args(args)
        .output()
        .unwrap()
}

fn first_line(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout)
        .lines()
        .next()
        .unwrap_or("")
        .to_string()
}

/// `classify --json` at a non-truncating pair budget: (consistent, unsatisfiable local
/// names, `completeness_guaranteed`, `dropped` empty). Asserts `incomplete: false`, so a
/// deadline cut cannot pass for an answer.
fn classify(f: &Fixture) -> (bool, Vec<String>, bool, bool) {
    let o = run(&["classify", "--json", "--pair-timeout-ms", "2000", f.path()]);
    assert!(o.status.success(), "classify failed: {o:?}");
    let j: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(j["incomplete"], false, "{}: {j}", f.path());
    let unsat: Vec<String> = j["unsatisfiable"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| {
            v.as_str()
                .unwrap()
                .trim_start_matches("http://ex/#")
                .to_owned()
        })
        .collect();
    (
        j["consistent"].as_bool().unwrap(),
        unsat,
        j["completeness_guaranteed"].as_bool().unwrap(),
        j["dropped"]
            .as_object()
            .is_some_and(serde_json::Map::is_empty),
    )
}

const NEGATIVE: &[(&str, &str)] = &[
    // An empty sub-role with no edge.
    (
        "n1",
        "SubObjectPropertyOf(:r owl:bottomObjectProperty) ClassAssertion(:C :a)",
    ),
    // Bottom BELOW an ordinary role says nothing about that role.
    (
        "n2",
        "SubObjectPropertyOf(owl:bottomObjectProperty :r) SubClassOf(:C ObjectSomeValuesFrom(:r :D))",
    ),
    // A universal over the empty role is vacuous; it gives neither unsat nor C ⊑ D.
    (
        "n3",
        "SubClassOf(:C ObjectAllValuesFrom(owl:bottomObjectProperty :D))",
    ),
    (
        "n4",
        "SubDataPropertyOf(:p owl:bottomDataProperty) ClassAssertion(:C :a)",
    ),
    // Negating an edge of the empty role is a tautology.
    (
        "n5",
        "NegativeObjectPropertyAssertion(owl:bottomObjectProperty :a :b) ClassAssertion(:C :a)",
    ),
    // A role BELOW top is not universal: negating its edge is fine.
    (
        "n6",
        "SubObjectPropertyOf(:r owl:topObjectProperty) NegativeObjectPropertyAssertion(:r :a :b) \
         ClassAssertion(:C :a)",
    ),
    // A chain with an ordinary leg does not make its super-role universal. Semantically
    // it empties s (`top(x, x) ∘ s(x, y)` gives `r(x, y)`), which rustdl does not derive
    // yet: `n7edge` in `universal_role_cli.rs` (#543 part 2). Here s has no edge.
    (
        "n7",
        "SubObjectPropertyOf(ObjectPropertyChain(owl:topObjectProperty :s) :r) \
         SubObjectPropertyOf(:r owl:bottomObjectProperty) ClassAssertion(:C :a)",
    ),
    // The empty-role closure goes down only and skips chain legs, with top interned
    // (Fable's code review, finding 1): none of these is a contradiction.
    (
        "n8",
        "SubObjectPropertyOf(owl:topObjectProperty :r) SubObjectPropertyOf(owl:bottomObjectProperty :r) \
         ClassAssertion(:C :a)",
    ),
    (
        "n9",
        "SubObjectPropertyOf(owl:topObjectProperty :r) \
         SubObjectPropertyOf(ObjectPropertyChain(:r :s) owl:bottomObjectProperty) ClassAssertion(:C :a)",
    ),
    (
        "n10",
        "SubObjectPropertyOf(owl:topObjectProperty :r) SubObjectPropertyOf(:s :r) \
         SubObjectPropertyOf(:s owl:bottomObjectProperty) ClassAssertion(:C :a)",
    ),
    // owl:topDataProperty is allowed only as a super-property in OWL 2 DL; both oracles
    // call this consistent, so no contradiction is stated.
    (
        "t6",
        "SubDataPropertyOf(owl:topDataProperty owl:bottomDataProperty) ClassAssertion(:C :a)",
    ),
];

#[test]
fn negative_controls_stay_consistent_and_satisfiable() {
    for (tag, body) in NEGATIVE {
        let f = Fixture::new(tag, body);
        assert_eq!(
            first_line(&run(&["consistent", f.path()])),
            "consistent",
            "{tag}"
        );
        let (consistent, unsat, _, dropped_empty) = classify(&f);
        assert!(
            consistent && unsat.is_empty() && dropped_empty,
            "{tag}: {unsat:?}"
        );
        assert_eq!(
            first_line(&run(&["sat", f.path(), "http://ex/#C"])),
            "sat",
            "{tag}"
        );
        let i = run(&["instance", f.path(), "http://ex/#D", "http://ex/#a"]);
        assert_eq!(first_line(&i), "no", "{tag}");
        assert!(run(&["realize", f.path()]).status.success(), "{tag}");
    }
}

/// n3 is vacuous: `C ⊑ ∀bottom.D` must not yield `C ⊑ D`.
#[test]
fn a_universal_over_the_empty_role_gives_no_subsumption() {
    let f = Fixture::new("n3-sub", NEGATIVE[2].1);
    let o = run(&["subclass", f.path(), "http://ex/#C", "http://ex/#D"]);
    assert_eq!(first_line(&o), "no");
}

/// `C` is unsatisfiable. The pure-EL shapes keep the saturation fast path, and its
/// completeness claim is now true.
const TBOX_UNSAT: &[(&str, &str, bool)] = &[
    (
        "p1",
        "SubClassOf(:C ObjectSomeValuesFrom(owl:bottomObjectProperty :D))",
        true,
    ),
    (
        "p2",
        "SubObjectPropertyOf(:r owl:bottomObjectProperty) SubClassOf(:C ObjectSomeValuesFrom(:r :D))",
        true,
    ),
    (
        "p3",
        "SubObjectPropertyOf(:s :r) SubObjectPropertyOf(:r owl:bottomObjectProperty) \
         SubClassOf(:C ObjectSomeValuesFrom(:s :D))",
        true,
    ),
    // Not EL: the tableau decides it.
    (
        "p4",
        "SubClassOf(:C ObjectUnionOf(ObjectSomeValuesFrom(owl:bottomObjectProperty :D) \
         ObjectSomeValuesFrom(owl:bottomObjectProperty :E)))",
        false,
    ),
    (
        "p9",
        "SubClassOf(:C ObjectHasSelf(owl:bottomObjectProperty))",
        false,
    ),
    (
        "d1",
        "SubClassOf(:C DataSomeValuesFrom(owl:bottomDataProperty rdfs:Literal))",
        true,
    ),
    (
        "d2",
        "SubDataPropertyOf(:p owl:bottomDataProperty) SubClassOf(:C DataSomeValuesFrom(:p xsd:integer))",
        true,
    ),
];

#[test]
fn an_existential_over_an_empty_role_is_unsatisfiable() {
    for (tag, body, guaranteed) in TBOX_UNSAT {
        let f = Fixture::new(tag, body);
        let (consistent, unsat, cg, dropped_empty) = classify(&f);
        assert!(consistent && dropped_empty, "{tag}");
        assert_eq!(unsat, vec!["C".to_owned()], "{tag}");
        if *guaranteed {
            assert!(
                cg,
                "{tag}: the pure-EL closure should be certified complete"
            );
        }
        assert_eq!(
            first_line(&run(&["sat", f.path(), "http://ex/#C"])),
            "unsat",
            "{tag}"
        );
    }
}

/// Every one of these is inconsistent: an edge of an empty role, or a universal role told
/// empty or negatively asserted. Each surface must say so; `realize` / `instance` /
/// `instances` refuse with the inconsistent-KB error instead of a definite answer.
const INCONSISTENT: &[(&str, &str)] = &[
    (
        "p5",
        "ObjectPropertyAssertion(owl:bottomObjectProperty :a :b) ClassAssertion(:C :a)",
    ),
    (
        "p6",
        "SubObjectPropertyOf(:r owl:bottomObjectProperty) ObjectPropertyAssertion(:r :a :b) \
         ClassAssertion(:C :a)",
    ),
    (
        "p7",
        "InverseObjectProperties(:r :s) SubObjectPropertyOf(:s owl:bottomObjectProperty) \
         ObjectPropertyAssertion(:r :a :b) ClassAssertion(:C :a)",
    ),
    (
        "p8",
        "SubObjectPropertyOf(ObjectPropertyChain(:r :s) owl:bottomObjectProperty) \
         ObjectPropertyAssertion(:r :a :b) ObjectPropertyAssertion(:s :b :c) ClassAssertion(:C :a)",
    ),
    // The edge is generated by the TBox, not asserted.
    (
        "p10",
        "SubObjectPropertyOf(:r owl:bottomObjectProperty) SubClassOf(:C ObjectSomeValuesFrom(:r :D)) \
         ClassAssertion(:C :a)",
    ),
    (
        "d3",
        "DataPropertyAssertion(owl:bottomDataProperty :a \"x\") ClassAssertion(:C :a)",
    ),
    (
        "d4",
        "SubDataPropertyOf(:p owl:bottomDataProperty) \
         DataPropertyAssertion(:p :a \"1\"^^xsd:integer) ClassAssertion(:C :a)",
    ),
    (
        "t1",
        "SubObjectPropertyOf(owl:topObjectProperty owl:bottomObjectProperty) ClassAssertion(:C :a)",
    ),
    (
        "t2",
        "SubObjectPropertyOf(owl:topObjectProperty :r) SubObjectPropertyOf(:r owl:bottomObjectProperty) \
         ClassAssertion(:C :a)",
    ),
    (
        "t3",
        "NegativeObjectPropertyAssertion(owl:topObjectProperty :a :b) ClassAssertion(:C :a)",
    ),
    (
        "t4",
        "SubObjectPropertyOf(owl:topObjectProperty :r) NegativeObjectPropertyAssertion(:r :a :b) \
         ClassAssertion(:C :a)",
    ),
    (
        "t5",
        "SubObjectPropertyOf(ObjectPropertyChain(owl:topObjectProperty owl:topObjectProperty) :r) \
         SubObjectPropertyOf(:r owl:bottomObjectProperty) ClassAssertion(:C :a)",
    ),
];

#[test]
fn an_edge_of_an_empty_role_is_inconsistent_on_every_surface() {
    for (tag, body) in INCONSISTENT {
        let f = Fixture::new(tag, body);
        assert_eq!(
            first_line(&run(&["consistent", f.path()])),
            "inconsistent",
            "{tag}"
        );
        let (consistent, _, _, _) = classify(&f);
        assert!(!consistent, "{tag}");
        for args in [
            vec!["realize", f.path()],
            vec!["instance", f.path(), "http://ex/#D", "http://ex/#a"],
            vec!["instances", f.path(), "http://ex/#D"],
        ] {
            let o = run(&args);
            let err = String::from_utf8_lossy(&o.stderr);
            assert!(
                !o.status.success() && err.contains("ontology is inconsistent"),
                "{tag} {}: status {:?}, stdout {:?}, stderr {err}",
                args[0],
                o.status,
                String::from_utf8_lossy(&o.stdout)
            );
        }
    }
}

/// Condition 1 of the #539 plan: the axiom conversion adds is never presented as the
/// user's. Justifications cite only the file's axioms, `diagnose` likewise, `prove`'s
/// provenance lists no built-in axiom as a source axiom, and nothing reads as dropped.
#[test]
fn the_builtin_axiom_never_appears_as_a_user_axiom() {
    let fx = Fixture::new(
        "prov",
        "SubClassOf(:E :C) SubClassOf(:C ObjectSomeValuesFrom(owl:bottomObjectProperty :D))",
    );
    let just = run(&["justify", fx.path(), "unsat", "http://ex/#E"]);
    let text = String::from_utf8_lossy(&just.stdout);
    let cited: Vec<&str> = text.lines().filter(|l| l.starts_with("  ")).collect();
    assert_eq!(cited.len(), 2, "{text}");
    assert!(!text.contains("owl:Thing"), "{text}");

    let diag = run(&["diagnose", fx.path()]);
    let text = String::from_utf8_lossy(&diag.stdout);
    assert!(
        text.contains("SubClassOf owl:bottomObjectProperty some"),
        "{text}"
    );
    assert!(!text.contains("owl:Thing"), "{text}");

    // `prove --json`: no step lists the built-in `∃bottom.⊤ ⊑ ⊥` among its SOURCE axioms
    // (it may only appear under `builtin_axioms`).
    let prove = run(&[
        "prove",
        "--json",
        fx.path(),
        "http://ex/#E",
        "http://www.w3.org/2002/07/owl#Nothing",
    ]);
    let pj: serde_json::Value = serde_json::from_slice(&prove.stdout).unwrap();
    fn source_axioms(n: &serde_json::Value, out: &mut Vec<String>) {
        if let Some(a) = n["axioms"].as_array() {
            out.extend(a.iter().map(|x| x.as_str().unwrap().to_owned()));
        }
        if let Some(ps) = n["premises"].as_array() {
            for q in ps {
                source_axioms(q, out);
            }
        }
    }
    let mut cited = Vec::new();
    source_axioms(&pj["proof"], &mut cited);
    // `prove --json` writes ⊤ as `ObjectIntersectionOf()`.
    assert!(
        cited.iter().all(|a| !a
            .contains("ObjectSomeValuesFrom(owl:bottomObjectProperty ObjectIntersectionOf())")),
        "built-in axiom cited as a source axiom: {cited:?}"
    );

    let verify = run(&["verify-el", "--json", fx.path()]);
    let vj: serde_json::Value = serde_json::from_slice(&verify.stdout).unwrap();
    assert_eq!(vj["dropped"], serde_json::json!({}), "{vj}");
    assert_eq!(vj["violations"], serde_json::json!([]), "{vj}");

    let (_, _, _, dropped_empty) = classify(&fx);
    assert!(dropped_empty);
}

/// Known limitation (`docs/known-limitations/bottom-data-property-unsupported-literal.md`):
/// a data assertion whose literal has an unsupported datatype is dropped whole, so the
/// emptiness of `p ⊑ owl:bottomDataProperty` never meets it, and the file reads
/// consistent (both oracles: inconsistent). The drop is flagged. This pins today's
/// behaviour; flip it when the role half (`(∃p.⊤)(a)`) is salvaged.
#[test]
fn known_limitation_an_unsupported_literal_under_bottom_data_is_dropped_and_flagged() {
    let f = Fixture::new(
        "residual",
        "SubDataPropertyOf(:p owl:bottomDataProperty) \
         DataPropertyAssertion(:p :a \"2020-01-01T00:00:00Z\"^^xsd:dateTimeStamp)",
    );
    let o = run(&["consistent", f.path()]);
    assert_eq!(first_line(&o), "consistent");
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(err.contains("not understood and dropped"), "{err}");
}
