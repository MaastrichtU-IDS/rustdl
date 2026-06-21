//! Integration tests for laconic justifications.

use horned_owl::model::{
    Build, ClassExpression as CE, Component, DeclareClass, DeclareObjectProperty, MutableOntology,
    SubClassOf,
};
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::find_laconic_justification;
use owl_dl_reasoner::justify::Entailment;

// A ⊑ B ⊓ C ⊓ ∃r.D ; query A ⊑ B  → laconic must be exactly {A ⊑ B}.
#[test]
fn laconic_drops_superfluous_conjuncts() {
    let b = Build::new_rc();
    let cls = |iri: &str| CE::Class(b.class(iri));
    let mut o = SetOntology::new();
    for c in ["urn:A", "urn:B", "urn:C", "urn:D"] {
        o.insert(DeclareClass(b.class(c)));
    }
    o.insert(DeclareObjectProperty(b.object_property("urn:r")));
    let some_rd = CE::ObjectSomeValuesFrom {
        ope: b.object_property("urn:r").into(),
        bce: Box::new(cls("urn:D")),
    };
    o.insert(SubClassOf {
        sub: cls("urn:A"),
        sup: CE::ObjectIntersectionOf(vec![cls("urn:B"), cls("urn:C"), some_rd]),
    });

    let q = Entailment::SubClassOf {
        sub: "urn:A".to_string(),
        sup: "urn:B".to_string(),
    };
    let lac = find_laconic_justification(&o, &q)
        .expect("laconic")
        .expect("entailed");
    let want = Component::SubClassOf(SubClassOf {
        sub: cls("urn:A"),
        sup: cls("urn:B"),
    });
    assert_eq!(lac.axioms, vec![want], "laconic must keep only A ⊑ B");
}

// C ≡ D ⊓ E ; query C ⊑ D  → laconic exactly {C ⊑ D}.
#[test]
fn laconic_equivalence_picks_one_direction_and_conjunct() {
    let b = Build::new_rc();
    let cls = |iri: &str| CE::Class(b.class(iri));
    let mut o = SetOntology::new();
    for c in ["urn:C", "urn:D", "urn:E"] {
        o.insert(DeclareClass(b.class(c)));
    }
    o.insert(horned_owl::model::EquivalentClasses(vec![
        cls("urn:C"),
        CE::ObjectIntersectionOf(vec![cls("urn:D"), cls("urn:E")]),
    ]));
    let q = Entailment::SubClassOf {
        sub: "urn:C".to_string(),
        sup: "urn:D".to_string(),
    };
    let lac = find_laconic_justification(&o, &q)
        .expect("laconic")
        .expect("entailed");
    let want = Component::SubClassOf(SubClassOf {
        sub: cls("urn:C"),
        sup: cls("urn:D"),
    });
    assert_eq!(lac.axioms, vec![want]);
}

// A ⊑ B, query A ⊑ Z (Z declared but unrelated) → not entailed → None.
#[test]
fn laconic_not_entailed_is_none() {
    let b = Build::new_rc();
    let cls = |iri: &str| CE::Class(b.class(iri));
    let mut o = SetOntology::new();
    for c in ["urn:A", "urn:B", "urn:Z"] {
        o.insert(DeclareClass(b.class(c)));
    }
    o.insert(SubClassOf {
        sub: cls("urn:A"),
        sup: cls("urn:B"),
    });
    let q = Entailment::SubClassOf {
        sub: "urn:A".to_string(),
        sup: "urn:Z".to_string(),
    };
    assert!(
        find_laconic_justification(&o, &q)
            .expect("laconic")
            .is_none()
    );
}

use owl_dl_reasoner::justify::find_one_justification;

type Rc = std::rc::Rc<str>;

// On a real fixture, the laconic justification of a known unsat class must be
// non-empty and still entail (QuickXplain guarantees the latter by construction).
// Ignored by default (reads the curated corpus). Reports the axiom/fragment counts.
#[test]
#[ignore = "reads the curated corpus (ontologies/real/pizza.ofn)"]
fn laconic_pizza_unsat_reports_fragments() {
    let p = std::path::Path::new("../../ontologies/real/pizza.ofn");
    if !p.exists() {
        eprintln!("skip pizza.ofn (not present)");
        return;
    }
    let onto = read_ofn_fixture(p);
    let q = Entailment::Unsatisfiable {
        class: "http://www.co-ode.org/ontologies/pizza/pizza.owl#CheeseyVegetableTopping"
            .to_string(),
    };
    let regular = find_one_justification(&onto, &q)
        .expect("justify")
        .expect("entailed");
    let lac = find_laconic_justification(&onto, &q)
        .expect("laconic")
        .expect("entailed");
    assert!(!lac.axioms.is_empty(), "laconic must be non-empty");
    eprintln!(
        "pizza CheeseyVegetableTopping: regular {} axioms, laconic {} fragments",
        regular.axioms.len(),
        lac.axioms.len()
    );
}

fn read_ofn_fixture(p: &std::path::Path) -> SetOntology<Rc> {
    use horned_owl::io::ParserConfiguration;
    use horned_owl::io::ofn::reader::read as read_ofn;
    let mut reader = std::io::BufReader::new(std::fs::File::open(p).expect("open fixture"));
    let (o, _): (SetOntology<Rc>, _) =
        read_ofn(&mut reader, ParserConfiguration::default()).expect("parse ofn");
    o
}
