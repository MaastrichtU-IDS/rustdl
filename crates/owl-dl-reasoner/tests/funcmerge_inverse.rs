//! Regression: functional (≤1) role merge across an inverse-induced edge, in a
//! cyclic model. Konclude derives A ⊑ Y (⊑ Z); rustdl missed it because the
//! wedge's ≤n-successor count ignored inverse-induced successors. See
//! docs/superpowers/specs/2026-07-11-funcmerge-inverse-completeness-design.md.

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::classify;

const FUNCMERGE_CYCLIC: &str = r#"Prefix(:=<http://t/#>)
Ontology(
Declaration(Class(:A))
Declaration(Class(:N))
Declaration(Class(:Y))
Declaration(Class(:Z))
Declaration(Class(:LFC))
Declaration(ObjectProperty(:f))
Declaration(ObjectProperty(:g))
Declaration(ObjectProperty(:h))
SubClassOf(:A ObjectSomeValuesFrom(:f :N))
InverseObjectProperties(:f :g)
FunctionalObjectProperty(:g)
EquivalentClasses(:N ObjectSomeValuesFrom(:g ObjectIntersectionOf(:Y ObjectSomeValuesFrom(:h :LFC))))
SubClassOf(:Y :Z)
EquivalentClasses(:LFC ObjectSomeValuesFrom(:g :A))
)
"#;

fn load(src: &str) -> SetOntology<RcStr> {
    let mut cur = std::io::Cursor::new(src.to_string());
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut cur, ParserConfiguration::default()).expect("parse OFN");
    onto
}

#[test]
fn funcmerge_cyclic_derives_a_sub_y() {
    let onto = load(FUNCMERGE_CYCLIC);
    let c = classify(&onto).expect("classify");
    // A ⊑ Y by the functional merge across the inverse edge; A ⊑ Z since Y ⊑ Z.
    assert!(
        c.is_subclass("http://t/#A", "http://t/#Y"),
        "expected A ⊑ Y (functional-merge-across-inverse)"
    );
    assert!(
        c.is_subclass("http://t/#A", "http://t/#Z"),
        "expected A ⊑ Z (A ⊑ Y ⊑ Z)"
    );
}
