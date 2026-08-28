use owl_dl_core::convert_ontology;
use owl_dl_verify::Interpretation;
use owl_dl_verify::model::FiniteModel;

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
