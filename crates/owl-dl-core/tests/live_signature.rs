//! Live-signature reporting: an entity is reportable only while some LIVE
//! axiom still mentions it (spec §4a).

use horned_owl::model::{
    AnnotatedComponent, Build, ClassExpression, Component, MutableOntology, RcStr, SubClassOf,
};
use horned_owl::ontology::set::SetOntology;
use owl_dl_core::convert::convert_ontology;
use owl_dl_core::ontology::InternalOntology;
use owl_dl_core::signature;
use owl_dl_core::{Axiom, Role};

#[test]
fn dropping_the_last_axiom_mentioning_a_class_drops_it_from_the_live_signature() {
    let b = Build::new_rc();
    let mut o = SetOntology::<RcStr>::new();
    o.insert(AnnotatedComponent::from(Component::SubClassOf(
        SubClassOf {
            sub: ClassExpression::Class(b.class("http://x/A")),
            sup: ClassExpression::Class(b.class("http://x/C")),
        },
    )));

    let mut internal = convert_ontology(&o).expect("convert");
    let a_id = internal
        .vocabulary
        .class_id("http://x/A")
        .expect("A interned");

    let sig = signature::compute(&internal);
    assert!(sig.has_class(a_id), "A is mentioned by a live axiom");

    // Kill every live axiom; A must drop out of the signature but keep its id.
    for i in internal.live_axiom_indices().collect::<Vec<_>>() {
        internal.kill_axiom(i);
    }
    let sig = signature::compute(&internal);
    assert!(
        !sig.has_class(a_id),
        "A is no longer mentioned by any live axiom"
    );
    // Id still resolves - ids are never recycled, only hidden.
    assert_eq!(internal.vocabulary.class_iri(a_id), "http://x/A");
}

#[test]
fn a_declaration_alone_keeps_an_entity_in_the_live_signature() {
    let mut o = InternalOntology::new();
    let c = o.vocabulary.intern_class("http://x/C");
    let r = o.vocabulary.intern_role("http://x/r");
    let a = o.vocabulary.intern_individual("http://x/a");
    let ic = o.push_live_axiom(Axiom::DeclareClass(c));
    let ir = o.push_live_axiom(Axiom::DeclareObjectProperty(r));
    let ia = o.push_live_axiom(Axiom::DeclareNamedIndividual(a));

    // A declaration is what keeps an otherwise-unused entity reportable.
    let sig = signature::compute(&o);
    assert!(sig.has_class(c), "DeclareClass references C");
    assert!(sig.has_role(r), "DeclareObjectProperty references r");
    assert!(sig.has_individual(a), "DeclareNamedIndividual references a");

    for i in [ic, ir, ia] {
        assert!(o.kill_axiom(i));
    }
    let sig = signature::compute(&o);
    assert!(!sig.has_class(c));
    assert!(!sig.has_role(r));
    assert!(!sig.has_individual(a));
}

#[test]
fn nested_concepts_mark_the_individuals_and_roles_they_mention() {
    let mut o = InternalOntology::new();
    let cls = o.vocabulary.intern_class("http://x/C");
    let role_r = o.vocabulary.intern_role("http://x/r");
    let role_s = o.vocabulary.intern_role("http://x/s");
    let ind = o.vocabulary.intern_individual("http://x/a");

    // C ⊑ ∃r.(¬{a} ⊓ Self(s⁻)) - the class is reachable only through the
    // nested expression, and the individual/roles only through Nominal and
    // SelfRestriction.
    let nominal = o.concepts.nominal(ind);
    let not_nominal = o.concepts.not(nominal);
    let self_s = o.concepts.self_restriction(Role::Inverse(role_s));
    let conj = o.concepts.and([not_nominal, self_s]);
    let filler = o.concepts.some(Role::Named(role_r), conj);
    let top = o.concepts.top();
    let idx = o.push_live_axiom(Axiom::SubClassOf {
        sub: top,
        sup: filler,
    });
    let atomic_c = o.concepts.atomic(cls);
    let idx_c = o.push_live_axiom(Axiom::SubClassOf {
        sub: atomic_c,
        sup: top,
    });

    let sig = signature::compute(&o);
    assert!(sig.has_class(cls));
    assert!(sig.has_role(role_r), "role under Some is marked");
    assert!(
        sig.has_role(role_s),
        "role under an inverse SelfRestriction is marked"
    );
    assert!(
        sig.has_individual(ind),
        "individual under Nominal is marked"
    );

    assert!(o.kill_axiom(idx));
    assert!(o.kill_axiom(idx_c));
    let sig = signature::compute(&o);
    assert!(!sig.has_class(cls));
    assert!(!sig.has_role(role_r));
    assert!(!sig.has_role(role_s));
    assert!(!sig.has_individual(ind));
}
