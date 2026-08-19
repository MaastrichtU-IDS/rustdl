use owl_dl_core::Axiom;
use owl_dl_core::ontology::InternalOntology;

fn top_bot_sub(o: &mut InternalOntology) -> Axiom {
    let t = o.concepts.top();
    let b = o.concepts.bot();
    Axiom::SubClassOf { sub: b, sup: t }
}

#[test]
fn killing_an_axiom_preserves_indices_of_survivors() {
    let mut o = InternalOntology::new();
    let a0 = top_bot_sub(&mut o);
    let a1 = top_bot_sub(&mut o);
    let i0 = o.push_live_axiom(a0);
    let i1 = o.push_live_axiom(a1);
    assert_eq!((i0, i1), (0, 1));
    assert_eq!(o.num_live_axioms(), 2);

    assert!(o.kill_axiom(i0));
    // Index of the survivor is unchanged - this is the whole point.
    assert_eq!(o.live_axiom_indices().collect::<Vec<_>>(), vec![i1]);
    assert_eq!(o.num_live_axioms(), 1);
    // The dead slot is still addressable so parallel provenance vectors stay valid.
    assert_eq!(o.axioms.len(), 2);
    // Killing twice is a no-op, not a panic or a double-decrement.
    assert!(!o.kill_axiom(i0));
    assert_eq!(o.num_live_axioms(), 1);
}

#[test]
fn axioms_pushed_by_convert_are_live_by_default() {
    let mut o = InternalOntology::new();
    let a = top_bot_sub(&mut o);
    o.axioms.push(a); // legacy direct push, as convert_ontology does today
    o.sync_liveness();
    assert_eq!(o.num_live_axioms(), 1);
}
