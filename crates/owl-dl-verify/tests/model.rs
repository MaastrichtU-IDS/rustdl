use owl_dl_core::convert_ontology;
use owl_dl_verify::Interpretation;
use owl_dl_verify::model::{FiniteModel, build_role_hierarchy, effective_ranges};

fn load(ofn: &str) -> owl_dl_core::InternalOntology {
    let (onto, _): (
        horned_owl::ontology::set::SetOntology<horned_owl::model::RcStr>,
        _,
    ) = horned_owl::io::ofn::reader::read(
        &mut ofn.as_bytes(),
        horned_owl::io::ParserConfiguration::default(),
    )
    .expect("parse fixture");
    convert_ontology(&onto).expect("convert fixture")
}

const CHAIN: &str = r"Prefix(:=<http://ex.org/>)
Ontology(<http://ex.org/t>
Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C))
SubClassOf(:A :B)
SubClassOf(:B :C)
)
";

#[test]
fn seeding_labels_are_sorted_and_contain_the_class_itself() {
    let internal = load(CHAIN);
    let (subs, facts, _) = owl_dl_saturation::saturate_with_exists_facts(&internal);
    let model = FiniteModel::seed(&internal, &subs, &facts);

    let a = internal
        .vocabulary
        .class_id("http://ex.org/A")
        .expect("A declared");
    let e = model
        .element_of_class(a)
        .expect("A is satisfiable, so it is seeded");
    let label = model.label(e);

    assert!(
        label.windows(2).all(|w| w[0] <= w[1]),
        "labels must be sorted: {label:?}"
    );
    assert!(
        label.contains(&a),
        "subsumers_of is reflexive, so A must be in its own label"
    );
    assert!(
        model.in_concept(e, a),
        "in_concept must agree with the label"
    );
}

#[test]
fn derived_equivalent_classes_share_one_element() {
    // B and C are NOT equivalent here, so they must be distinct elements.
    let internal = load(CHAIN);
    let (subs, facts, _) = owl_dl_saturation::saturate_with_exists_facts(&internal);
    let model = FiniteModel::seed(&internal, &subs, &facts);
    let b = internal.vocabulary.class_id("http://ex.org/B").expect("B");
    let c = internal.vocabulary.class_id("http://ex.org/C").expect("C");
    assert_ne!(
        model.element_of_class(b),
        model.element_of_class(c),
        "B and C have different subsumer sets, so they must not be interned together"
    );
}

#[test]
fn subsumers_of_is_reflexive_which_interning_and_the_witness_argument_both_need() {
    let internal = load(CHAIN);
    let (subs, _, _) = owl_dl_saturation::saturate_with_exists_facts(&internal);
    let a = internal.vocabulary.class_id("http://ex.org/A").expect("A");
    assert!(
        subs.subsumers_of(a).contains(&a),
        "spec §3 and the label-interning argument both consume reflexivity"
    );
}

const RANGES: &str = r"Prefix(:=<http://ex.org/>)
Ontology(<http://ex.org/r>
Declaration(Class(:F)) Declaration(Class(:G))
Declaration(ObjectProperty(:p)) Declaration(ObjectProperty(:q))
SubObjectPropertyOf(:p :q)
ObjectPropertyRange(:q :F)
ObjectPropertyRange(:p :G)
)
";

#[test]
fn effective_ranges_unions_over_super_roles() {
    let internal = load(RANGES);
    let hier = build_role_hierarchy(&internal);
    let er = effective_ranges(&internal, &hier);
    let p = internal.vocabulary.role_id("http://ex.org/p").expect("p");
    let q = internal.vocabulary.role_id("http://ex.org/q").expect("q");
    let f = internal.vocabulary.class_id("http://ex.org/F").expect("F");
    let g = internal.vocabulary.class_id("http://ex.org/G").expect("G");

    let pr = er.get(&p).cloned().unwrap_or_default();
    assert!(
        pr.contains(&f),
        "p ⊑ q, so Range(q,F) constrains p-successors"
    );
    assert!(
        pr.contains(&g),
        "p's own range must be included (super_roles is reflexive)"
    );
    let qr = er.get(&q).cloned().unwrap_or_default();
    assert!(qr.contains(&f));
    assert!(
        !qr.contains(&g),
        "a SUB-role's range must NOT leak upward to q"
    );
}
