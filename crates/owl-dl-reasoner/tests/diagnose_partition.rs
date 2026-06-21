//! Integration tests for `diagnose`: cascade fixture, inconsistency, conservation.

use horned_owl::model::{Build, MutableOntology};
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::diagnose;

type Rc = std::rc::Rc<str>;

fn b() -> Build<Rc> {
    Build::new_rc()
}

// Root = Bad (Bad ⊑ A ⊓ ¬A); Derived = SubBad (SubBad ⊑ Bad).
#[test]
fn root_and_derived_cascade() {
    let b = b();
    let mut o = SetOntology::new();
    use horned_owl::model::ClassExpression as CE;
    // Bad ⊑ A ⊓ ¬A  → Bad unsat (a root: depends on no other unsat class)
    o.insert(horned_owl::model::SubClassOf {
        sub: CE::Class(b.class("urn:Bad")),
        sup: CE::ObjectIntersectionOf(vec![
            CE::Class(b.class("urn:A")),
            CE::ObjectComplementOf(Box::new(CE::Class(b.class("urn:A")))),
        ]),
    });
    // SubBad ⊑ Bad  → SubBad unsat (derived from Bad)
    o.insert(horned_owl::model::SubClassOf {
        sub: CE::Class(b.class("urn:SubBad")),
        sup: CE::Class(b.class("urn:Bad")),
    });

    let d = diagnose(&o).expect("diagnose");
    assert!(d.consistent, "ontology is consistent (no ABox clash)");
    assert_eq!(d.roots, vec!["urn:Bad".to_string()]);
    assert_eq!(d.derived.len(), 1);
    assert_eq!(d.derived[0].iri, "urn:SubBad");
    assert_eq!(d.derived[0].roots, vec!["urn:Bad".to_string()]);
    // conservation
    let mut union: std::collections::BTreeSet<String> = d.roots.iter().cloned().collect();
    union.extend(d.derived.iter().map(|x| x.iri.clone()));
    let all: std::collections::BTreeSet<String> = d.all_unsat.iter().cloned().collect();
    assert_eq!(union, all);
}

// An ABox clash makes the ontology inconsistent: diagnose reports it, partition empty.
#[test]
fn inconsistent_ontology_flagged() {
    let b = b();
    let mut o = SetOntology::new();
    use horned_owl::model::ClassExpression as CE;
    // A DisjointWith B ; individual i is both A and B → inconsistent.
    o.insert(horned_owl::model::DisjointClasses(vec![
        CE::Class(b.class("urn:A")),
        CE::Class(b.class("urn:B")),
    ]));
    o.insert(horned_owl::model::ClassAssertion {
        ce: CE::Class(b.class("urn:A")),
        i: b.named_individual("urn:i").into(),
    });
    o.insert(horned_owl::model::ClassAssertion {
        ce: CE::Class(b.class("urn:B")),
        i: b.named_individual("urn:i").into(),
    });

    let d = diagnose(&o).expect("diagnose");
    assert!(!d.consistent, "ontology must be flagged inconsistent");
    assert!(d.roots.is_empty());
    assert!(d.derived.is_empty());
    assert!(d.all_unsat.is_empty());
}

// TBox global inconsistency (⊤ ⊑ ⊥): classify reports all classes unsat without
// flagging; the all-classes-unsat guard must escalate to `is_consistent` and
// report INCONSISTENT.
#[test]
fn tbox_global_inconsistency_flagged() {
    let b = b();
    let mut o = SetOntology::new();
    use horned_owl::model::ClassExpression as CE;
    // ⊤ ⊑ A ⊓ ¬A  → every class unsatisfiable; ontology inconsistent.
    o.insert(horned_owl::model::SubClassOf {
        sub: CE::Class(b.class("http://www.w3.org/2002/07/owl#Thing")),
        sup: CE::ObjectIntersectionOf(vec![
            CE::Class(b.class("urn:A")),
            CE::ObjectComplementOf(Box::new(CE::Class(b.class("urn:A")))),
        ]),
    });

    let d = diagnose(&o).expect("diagnose");
    assert!(
        !d.consistent,
        "⊤ ⊑ ⊥ ontology must be flagged inconsistent (all-classes-unsat guard)"
    );
    assert!(d.roots.is_empty() && d.derived.is_empty());
}

// A single class made empty (A ⊑ A ⊓ ¬A) is CONSISTENT (⊤ still satisfiable) even
// though every declared class is unsat. The all-classes-unsat guard must NOT
// false-flag it: `is_consistent` is the authoritative tiebreak → consistent, with
// A reported as a root.
#[test]
fn all_classes_empty_but_consistent_not_flagged() {
    let b = b();
    let mut o = SetOntology::new();
    use horned_owl::model::ClassExpression as CE;
    o.insert(horned_owl::model::SubClassOf {
        sub: CE::Class(b.class("urn:A")),
        sup: CE::ObjectIntersectionOf(vec![
            CE::Class(b.class("urn:A")),
            CE::ObjectComplementOf(Box::new(CE::Class(b.class("urn:A")))),
        ]),
    });

    let d = diagnose(&o).expect("diagnose");
    assert!(
        d.consistent,
        "{{A ⊑ ⊥}} is consistent (⊤ satisfiable); guard must not false-flag"
    );
    assert_eq!(d.roots, vec!["urn:A".to_string()]);
    assert!(d.derived.is_empty());
}

use owl_dl_reasoner::classify;

// Conservation invariant on real fixtures: roots ∪ derived == classified-unsat.
// pizza has designed-in unsatisfiable classes, so it exercises the partition on
// REAL data (not just the synthetic cascade). Ignored by default (reads the
// curated corpus); run with `-- --ignored`.
#[test]
#[ignore = "reads the curated corpus (ontologies/real/*.ofn)"]
fn corpus_conservation_invariant() {
    // cwd at test runtime is the crate dir; the corpus lives at the workspace
    // root (verified pattern, see real_ontology_corpus.rs uses `../../`).
    // pizza FIRST: it has unsat classes, so the partition is genuinely exercised.
    let mut exercised_nonempty = false;
    for path in [
        "../../ontologies/real/pizza.ofn",
        "../../ontologies/real/sio.ofn",
    ] {
        let p = std::path::Path::new(path);
        if !p.exists() {
            eprintln!("skip {path} (not present)");
            continue;
        }
        let onto = read_ofn_fixture(p);
        let classification = classify(&onto).expect("classify");
        let classified: std::collections::BTreeSet<String> = classification
            .unsatisfiable_classes()
            .into_iter()
            .map(str::to_string)
            .collect();

        let d = diagnose(&onto).expect("diagnose");
        if !d.consistent {
            eprintln!("{path}: inconsistent — partition deliberately empty, skipping conservation");
            continue;
        }
        let mut union: std::collections::BTreeSet<String> = d.roots.iter().cloned().collect();
        union.extend(d.derived.iter().map(|x| x.iri.clone()));
        assert_eq!(
            union, classified,
            "{path}: roots ∪ derived must equal the classified unsat set"
        );
        if !classified.is_empty() {
            exercised_nonempty = true;
            assert!(
                !d.roots.is_empty(),
                "{path}: unsat classes present but no root reported"
            );
        }
        eprintln!(
            "{path}: OK — {} unsat ({} root, {} derived)",
            classified.len(),
            d.roots.len(),
            d.derived.len()
        );
    }
    assert!(
        exercised_nonempty,
        "conservation test was vacuous — no fixture with unsat classes ran (is pizza present?)"
    );
}

// family is HermiT/Konclude-inconsistent; the ABox-saturation pre-check must flag
// it WITHOUT the slow classify/is_consistent path. Ignored (reads the corpus).
#[test]
#[ignore = "reads the curated corpus (ontologies/real/family.ofn)"]
fn family_inconsistency_flagged() {
    let p = std::path::Path::new("../../ontologies/real/family.ofn");
    if !p.exists() {
        eprintln!("skip family.ofn (not present)");
        return;
    }
    let onto = read_ofn_fixture(p);
    let d = diagnose(&onto).expect("diagnose");
    assert!(!d.consistent, "family must be flagged inconsistent");
    assert!(d.roots.is_empty() && d.derived.is_empty());
}

// Read a .ofn fixture into a SetOntology (verified pattern, see classify_inverse_domain.rs).
fn read_ofn_fixture(p: &std::path::Path) -> SetOntology<Rc> {
    use horned_owl::io::ParserConfiguration;
    use horned_owl::io::ofn::reader::read as read_ofn;
    let mut reader = std::io::BufReader::new(std::fs::File::open(p).expect("open fixture"));
    let (o, _): (SetOntology<Rc>, _) =
        read_ofn(&mut reader, ParserConfiguration::default()).expect("parse ofn");
    o
}
