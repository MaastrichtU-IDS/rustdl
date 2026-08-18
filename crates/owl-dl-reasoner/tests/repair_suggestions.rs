//! Integration tests for repair suggestions.

use horned_owl::model::{Build, ClassExpression as CE, DeclareClass, MutableOntology, SubClassOf};
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::justify::{
    Entailment, PreparedJustifier, entails, logical_axioms, ontology_from,
};
use owl_dl_reasoner::{find_repairs, find_repairs_prepared};

// X unsat via TWO independent justifications:
//   J1 = { X ⊑ A, X ⊑ B }   (A,B disjoint)
//   J2 = { X ⊑ C }          (C ⊑ ⊥ via C ⊑ D ⊓ ¬D)
// A repair must hit BOTH: e.g. {X⊑A, X⊑C} or {X⊑B, X⊑C}.
#[test]
fn repairs_hit_every_justification_and_verify() {
    let b = Build::new_rc();
    let cls = |iri: &str| CE::Class(b.class(iri));
    let mut o = SetOntology::new();
    for c in ["urn:X", "urn:A", "urn:B", "urn:C", "urn:D"] {
        o.insert(DeclareClass(b.class(c)));
    }
    o.insert(horned_owl::model::DisjointClasses(vec![
        cls("urn:A"),
        cls("urn:B"),
    ]));
    o.insert(SubClassOf {
        sub: cls("urn:X"),
        sup: cls("urn:A"),
    });
    o.insert(SubClassOf {
        sub: cls("urn:X"),
        sup: cls("urn:B"),
    });
    o.insert(SubClassOf {
        sub: cls("urn:C"),
        sup: CE::ObjectIntersectionOf(vec![
            cls("urn:D"),
            CE::ObjectComplementOf(Box::new(cls("urn:D"))),
        ]),
    });
    o.insert(SubClassOf {
        sub: cls("urn:X"),
        sup: cls("urn:C"),
    });

    let q = Entailment::Unsatisfiable {
        class: "urn:X".to_string(),
    };
    let r = find_repairs(&o, &q, 10).expect("repair");
    assert!(r.entailed, "X is unsatisfiable → entailed");
    assert!(!r.repairs.is_empty(), "must find at least one repair");

    let (fixed, logical) = logical_axioms(&o);
    for rep in &r.repairs {
        let kept: Vec<_> = logical
            .iter()
            .filter(|a| !rep.remove.contains(a))
            .cloned()
            .collect();
        let o2 = ontology_from(&fixed, &kept);
        assert!(
            !entails(&o2, &q).expect("entails"),
            "repair {:?} must break the unsatisfiability",
            rep.remove
        );
        assert!(
            rep.remove.len() >= 2,
            "must hit both independent justifications"
        );
    }
}

// Not entailed → entailed=false, no repairs.
#[test]
fn not_entailed_nothing_to_repair() {
    let b = Build::new_rc();
    let cls = |iri: &str| CE::Class(b.class(iri));
    let mut o = SetOntology::new();
    for c in ["urn:A", "urn:B"] {
        o.insert(DeclareClass(b.class(c)));
    }
    o.insert(SubClassOf {
        sub: cls("urn:A"),
        sup: cls("urn:B"),
    });
    let q = Entailment::Unsatisfiable {
        class: "urn:A".to_string(),
    };
    let r = find_repairs(&o, &q, 10).expect("repair");
    assert!(!r.entailed);
    assert!(r.repairs.is_empty());
}

type Rc = std::rc::Rc<str>;

// Real fixture: every repair of a pizza unsat class must verify (make it
// satisfiable). Ignored by default (corpus + SHOIN justify cost).
#[test]
#[ignore = "reads the curated corpus (ontologies/real/pizza.ofn)"]
fn repair_pizza_unsat_verifies() {
    let p = std::path::Path::new("../../ontologies/real/pizza.ofn");
    if !p.exists() {
        eprintln!("skip pizza.ofn (not present)");
        return;
    }
    let onto = read_ofn_fixture(p);
    let q = Entailment::Unsatisfiable {
        class: "http://www.co-ode.org/ontologies/pizza/pizza.owl#IceCream".to_string(),
    };
    let r = find_repairs(&onto, &q, 10).expect("repair");
    assert!(
        r.entailed && !r.repairs.is_empty(),
        "IceCream unsat → repairs exist"
    );
    let (fixed, logical) = logical_axioms(&onto);
    for rep in &r.repairs {
        let kept: Vec<_> = logical
            .iter()
            .filter(|a| !rep.remove.contains(a))
            .cloned()
            .collect();
        assert!(
            !entails(&ontology_from(&fixed, &kept), &q).expect("entails"),
            "every reported repair must break IceCream's unsatisfiability"
        );
    }
    eprintln!(
        "pizza IceCream: {} repair(s), complete={}",
        r.repairs.len(),
        r.complete
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

// `find_repairs` delegates to `find_repairs_prepared`, and `report` calls the
// prepared form directly with a justifier it also justifies from — so pin that the
// two entry points agree on every field, including the not-entailed verdict.
#[test]
fn prepared_repairs_match_the_one_shot_function() {
    let b = Build::new_rc();
    let cls = |iri: &str| CE::Class(b.class(iri));
    let mut o = SetOntology::new();
    for c in ["urn:X", "urn:A", "urn:B", "urn:C"] {
        o.insert(DeclareClass(b.class(c)));
    }
    o.insert(horned_owl::model::DisjointClasses(vec![
        cls("urn:A"),
        cls("urn:B"),
    ]));
    o.insert(SubClassOf {
        sub: cls("urn:X"),
        sup: cls("urn:A"),
    });
    o.insert(SubClassOf {
        sub: cls("urn:X"),
        sup: cls("urn:B"),
    });

    let prepared = PreparedJustifier::prepare(&o);
    for (label, q) in [
        (
            "entailed",
            Entailment::Unsatisfiable {
                class: "urn:X".to_string(),
            },
        ),
        (
            "not entailed",
            Entailment::Unsatisfiable {
                class: "urn:C".to_string(),
            },
        ),
    ] {
        let cold = find_repairs(&o, &q, 10).expect("repair");
        let warm = find_repairs_prepared(&prepared, &q, 10).expect("prepared repair");
        assert_eq!(cold.entailed, warm.entailed, "{label}: entailed");
        assert_eq!(cold.complete, warm.complete, "{label}: complete");
        assert_eq!(
            cold.dropped_unverified, warm.dropped_unverified,
            "{label}: dropped_unverified"
        );
        assert_eq!(cold.repairs, warm.repairs, "{label}: repairs");
    }
}
