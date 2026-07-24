#![allow(clippy::doc_markdown)]
//! Includes an external completeness/soundness oracle for `disjoint_classes`
//! (issue #47): rustdl's output is diffed against **HermiT**-inferred
//! `DisjointClasses` axioms. FP (rustdl reports, HermiT does not) is the hard
//! soundness guard and must always be empty; MISSED (HermiT infers, rustdl
//! omits) is allowed to be non-empty only when rustdl reports
//! `Disjointness::incomplete() == true`.
//!
//! The oracle is generated offline by `docker/robot/disjoint-oracle.sh`
//! (ROBOT + embedded HermiT) and committed as `dj-disjoint.owx`, so this test
//! needs no docker at run time.
//!
//! Regenerate after changing the fixture:
//!   bash docker/robot/disjoint-oracle.sh \
//!     crates/owl-dl-reasoner/tests/fixtures/disjoint/dj.ofn \
//!     crates/owl-dl-reasoner/tests/fixtures/disjoint/dj-disjoint.owx
//!
//! NOTE: `dj.ofn` is deliberately kept fully satisfiable/consistent — ROBOT's
//! `reason` command hard-errors (exit 1, no bypass flag) on ANY ontology
//! containing an unsatisfiable class, so it cannot serve as oracle input for a
//! fixture exercising unsat-exclusion. That behaviour is instead covered by
//! `disjoint_classes_excludes_unsatisfiable_classes` below, an inline-ontology
//! unit test (mirroring the other non-oracle tests in this file) that doesn't
//! go through ROBOT.
#![allow(clippy::unwrap_used)]
use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::io::owx::reader::read as read_owx;
use horned_owl::model::{ClassExpression, Component, RcStr};
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::disjoint_classes;
use std::collections::BTreeSet;
use std::fs::File;
use std::io::BufReader;
use std::io::Cursor;
use std::path::Path;

const THING: &str = "http://www.w3.org/2002/07/owl#Thing";
const NOTHING: &str = "http://www.w3.org/2002/07/owl#Nothing";

fn onto(src: &str) -> SetOntology<RcStr> {
    read_ofn(
        &mut Cursor::new(src.to_owned()),
        ParserConfiguration::default(),
    )
    .unwrap()
    .0
}

#[test]
fn disjoint_classes_inherits_through_subclass() {
    // A,B told disjoint; C⊑A, D⊑B ⇒ C,D entailed disjoint (not told).
    let o = onto(
        r"Prefix(:=<http://ex/#>)
          Ontology(<http://ex/>
            Declaration(Class(:A)) Declaration(Class(:B))
            Declaration(Class(:C)) Declaration(Class(:D))
            DisjointClasses(:A :B) SubClassOf(:C :A) SubClassOf(:D :B))",
    );
    let r = disjoint_classes(&o, None).unwrap();
    let has = |x: &str, y: &str| {
        r.pairs()
            .iter()
            .any(|(a, b)| (a == x && b == y) || (a == y && b == x))
    };
    assert!(has("http://ex/#A", "http://ex/#B"), "told pair present");
    assert!(
        has("http://ex/#C", "http://ex/#D"),
        "inherited pair inferred: {:?}",
        r.pairs()
    );
}

#[test]
fn disjoint_classes_errors_on_inconsistent() {
    let o = onto(
        r"Prefix(:=<http://ex/#>)
          Ontology(<http://ex/>
            Declaration(Class(:A)) Declaration(Class(:B)) Declaration(NamedIndividual(:x))
            DisjointClasses(:A :B) ClassAssertion(:A :x) ClassAssertion(:B :x))",
    );
    assert!(matches!(
        disjoint_classes(&o, None),
        Err(owl_dl_reasoner::ReasonError::Inconsistent)
    ));
}

#[test]
fn disjoint_object_properties_told() {
    let o = onto(
        r"Prefix(:=<http://ex/#>)
          Ontology(<http://ex/>
            Declaration(ObjectProperty(:p)) Declaration(ObjectProperty(:q))
            DisjointObjectProperties(:p :q))",
    );
    let pairs = owl_dl_reasoner::disjoint_object_properties(&o).unwrap();
    assert_eq!(
        pairs,
        vec![("http://ex/#p".to_string(), "http://ex/#q".to_string())]
    );
}

#[test]
fn disjoint_data_properties_told() {
    let o = onto(
        r"Prefix(:=<http://ex/#>)
          Ontology(<http://ex/>
            Declaration(DataProperty(:p)) Declaration(DataProperty(:q))
            DisjointDataProperties(:p :q))",
    );
    let pairs = owl_dl_reasoner::disjoint_data_properties(&o).unwrap();
    assert_eq!(
        pairs,
        vec![("http://ex/#p".to_string(), "http://ex/#q".to_string())]
    );
}

/// Ghost is equivalent to `Animal ⊓ Plant` (told-disjoint) and so is
/// unsatisfiable; confirms `disjoint_classes` EXCLUDES an unsat class from its
/// output (reporting it disjoint with everything would be a degenerate,
/// unhelpful truth) rather than a ROBOT-fed oracle, since ROBOT's `reason`
/// hard-errors on any incoherent ontology (see the module doc comment).
#[test]
fn disjoint_classes_excludes_unsatisfiable_classes() {
    let o = onto(
        r"Prefix(:=<http://ex/#>)
          Ontology(<http://ex/>
            Declaration(Class(:Animal)) Declaration(Class(:Plant))
            Declaration(Class(:Ghost))
            DisjointClasses(:Animal :Plant)
            EquivalentClasses(:Ghost ObjectIntersectionOf(:Animal :Plant)))",
    );
    let r = disjoint_classes(&o, None).unwrap();
    assert!(
        r.pairs()
            .iter()
            .all(|(a, b)| !a.contains("Ghost") && !b.contains("Ghost")),
        "unsatisfiable class Ghost must not appear in any reported pair: {:?}",
        r.pairs()
    );
    // The told pair over the still-satisfiable classes is unaffected.
    assert!(r.pairs().iter().any(
        |(a, b)| (a == "http://ex/#Animal" && b == "http://ex/#Plant")
            || (a == "http://ex/#Plant" && b == "http://ex/#Animal")
    ));
}

/// HermiT-inferred `DisjointClasses` pairs between NAMED classes from the
/// committed oracle, decomposed pairwise from each (possibly n-ary)
/// `DisjointClasses` axiom, normalised to sorted `(lo, hi)`, and filtered of
/// `owl:Thing`/`owl:Nothing`.
fn oracle_disjoint_pairs(path: &Path) -> BTreeSet<(String, String)> {
    let file = File::open(path).unwrap_or_else(|e| panic!("open {}: {e}", path.display()));
    let mut reader = BufReader::new(file);
    let (onto, _): (SetOntology<RcStr>, _) = read_owx(&mut reader, ParserConfiguration::default())
        .unwrap_or_else(|e| panic!("parse {}: {e}", path.display()));
    let mut set = BTreeSet::new();
    for ax in &onto {
        if let Component::DisjointClasses(dc) = &ax.component {
            let names: Vec<String> =
                dc.0.iter()
                    .filter_map(|ce| match ce {
                        ClassExpression::Class(c) => Some(c.0.to_string()),
                        _ => None,
                    })
                    .filter(|iri| iri != THING && iri != NOTHING)
                    .collect();
            for i in 0..names.len() {
                for j in (i + 1)..names.len() {
                    let (a, b) = (&names[i], &names[j]);
                    let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
                    set.insert((lo.clone(), hi.clone()));
                }
            }
        }
    }
    set
}

#[test]
fn disjoint_classes_matches_hermit_oracle() {
    let dir = Path::new("tests/fixtures/disjoint");
    let file = File::open(dir.join("dj.ofn")).expect("fixture");
    let mut reader = BufReader::new(file);
    let (o, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut reader, ParserConfiguration::default()).expect("parse fixture");

    let result = disjoint_classes(&o, None).expect("disjoint_classes");
    let got: BTreeSet<(String, String)> = result.pairs().iter().cloned().collect();
    let oracle = oracle_disjoint_pairs(&dir.join("dj-disjoint.owx"));

    let fp: Vec<_> = got.difference(&oracle).collect();
    // Soundness guard (issue #47): rustdl must never report a disjoint pair
    // HermiT doesn't. Always asserted, regardless of completeness.
    assert!(
        fp.is_empty(),
        "FP — rustdl reports, HermiT does not: {fp:?}"
    );

    let missed: Vec<_> = oracle.difference(&got).collect();
    if result.incomplete() {
        // A sound under-approximation is acceptable when rustdl itself flags
        // the answer as incomplete.
        if !missed.is_empty() {
            eprintln!("MISSED (allowed, disjoint_classes reported incomplete): {missed:?}");
        }
    } else {
        assert!(
            missed.is_empty(),
            "MISSED — HermiT infers, rustdl omits (and rustdl did not report incomplete): {missed:?}"
        );
    }
}
