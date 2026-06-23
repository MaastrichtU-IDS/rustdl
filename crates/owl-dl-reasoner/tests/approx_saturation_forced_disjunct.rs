//! SP-A integration canaries: forced-disjunct resolves atomic disjunctions
//! end-to-end (via `convert_ontology` → saturation), without false positives.
#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::classify_top_down_with_timeout;
use std::io::Cursor;
use std::time::Duration;

fn classify(src: &str) -> owl_dl_reasoner::Classification {
    let mut r = Cursor::new(src.to_string().into_bytes());
    let (ont, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut r, ParserConfiguration::default()).expect("parse ofn");
    classify_top_down_with_timeout(&ont, Duration::from_secs(10)).expect("classify")
}

const BASE: &str = "Prefix(:=<http://t/>)\nOntology(<http://t/o>\n";

#[test]
fn forced_disjunct_resolves_to_survivor() {
    // C ⊑ A⊔B, C ⊑ G, Disjoint(G,A) ⟹ C ⊑ B should be entailed.
    let src = format!(
        "{BASE}\
Declaration(Class(:C)) Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:G))
SubClassOf(:C ObjectUnionOf(:A :B))
SubClassOf(:C :G)
DisjointClasses(:G :A)
)"
    );
    let cls = classify(&src);
    assert!(
        cls.is_subclass("http://t/C", "http://t/B"),
        "forced-disjunct: C ⊑ B must be entailed"
    );
}

#[test]
fn forced_to_bot_makes_unsat() {
    // C ⊑ A⊔B, C ⊑ G, Disjoint(G,A), Disjoint(G,B) ⟹ C unsatisfiable.
    let src = format!(
        "{BASE}\
Declaration(Class(:C)) Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:G))
SubClassOf(:C ObjectUnionOf(:A :B))
SubClassOf(:C :G)
DisjointClasses(:G :A)
DisjointClasses(:G :B)
)"
    );
    let cls = classify(&src);
    assert!(
        cls.unsatisfiable_classes().contains(&"http://t/C"),
        "forced-to-bot: C must be unsatisfiable"
    );
}

#[test]
fn undetermined_no_false_positive() {
    // C ⊑ A⊔B with no disjointness ⟹ neither C⊑A nor C⊑B may be entailed.
    let src = format!(
        "{BASE}\
Declaration(Class(:C)) Declaration(Class(:A)) Declaration(Class(:B))
SubClassOf(:C ObjectUnionOf(:A :B))
)"
    );
    let cls = classify(&src);
    assert!(
        !cls.is_subclass("http://t/C", "http://t/A"),
        "no spurious C⊑A"
    );
    assert!(
        !cls.is_subclass("http://t/C", "http://t/B"),
        "no spurious C⊑B"
    );
    assert!(
        !cls.unsatisfiable_classes().contains(&"http://t/C"),
        "C must stay satisfiable"
    );
}
