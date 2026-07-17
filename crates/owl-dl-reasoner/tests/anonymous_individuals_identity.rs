//! Soundness of anonymous-individual IDENTITY reasoning: interned anon
//! individuals participate in `SameAs` / `DifferentFrom` / functional-`≤1` merge /
//! disjointness exactly as named individuals. Verdicts are oracle-adjudicated
//! (HermiT/Konclude); see the anon-individuals plan Task 4.
use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::{instances_of, is_consistent};
use std::io::Cursor;

fn onto(body: &str) -> SetOntology<RcStr> {
    let src = format!("Prefix(:=<http://e#>)\nOntology(\n{body}\n)");
    let mut r = Cursor::new(src);
    read_ofn(&mut r, ParserConfiguration::default())
        .expect("parse ofn")
        .0
}

// A) anon instance of two disjoint classes ⇒ inconsistent.
#[test]
fn anon_in_two_disjoint_classes_is_inconsistent() {
    let o = onto(
        "Declaration(Class(:A)) Declaration(Class(:B)) DisjointClasses(:A :B)\n\
         ClassAssertion(:A _:x) ClassAssertion(:B _:x)",
    );
    assert!(
        !is_consistent(&o).expect("consistency"),
        "A⊓B on _:x must be inconsistent"
    );
}

// B) functional r + two anon witnesses of a's r, one A one B (A,B disjoint) ⇒
//    merge forces A⊓B ⇒ inconsistent.
#[test]
fn functional_merges_anon_witnesses_into_clash() {
    let o = onto(
        "Declaration(Class(:A)) Declaration(Class(:B)) DisjointClasses(:A :B)\n\
         Declaration(ObjectProperty(:r)) FunctionalObjectProperty(:r)\n\
         Declaration(NamedIndividual(:a))\n\
         ObjectPropertyAssertion(:r :a _:x) ObjectPropertyAssertion(:r :a _:y)\n\
         ClassAssertion(:A _:x) ClassAssertion(:B _:y)",
    );
    assert!(
        !is_consistent(&o).expect("consistency"),
        "functional merge of _:x,_:y into A⊓B must clash"
    );
}

// C) functional r + two anon witnesses asserted DifferentIndividuals ⇒ ≤1 clash.
#[test]
fn functional_plus_different_anon_witnesses_is_inconsistent() {
    let o = onto(
        "Declaration(ObjectProperty(:r)) FunctionalObjectProperty(:r)\n\
         Declaration(NamedIndividual(:a))\n\
         ObjectPropertyAssertion(:r :a _:x) ObjectPropertyAssertion(:r :a _:y)\n\
         DifferentIndividuals(_:x _:y)",
    );
    assert!(
        !is_consistent(&o).expect("consistency"),
        "functional + ≠ anon witnesses must clash"
    );
}

// D) control: the same as C without DifferentIndividuals ⇒ consistent (they merge).
#[test]
fn functional_anon_witnesses_without_diff_is_consistent() {
    let o = onto(
        "Declaration(ObjectProperty(:r)) FunctionalObjectProperty(:r)\n\
         Declaration(NamedIndividual(:a))\n\
         ObjectPropertyAssertion(:r :a _:x) ObjectPropertyAssertion(:r :a _:y)",
    );
    assert!(
        is_consistent(&o).expect("consistency"),
        "functional anon witnesses without ≠ must merge (consistent)"
    );
}

// E) SameIndividual(:a, _:x): _:x∈A propagates to the NAMED :a ⇒ :a reported ∈ A.
#[test]
fn sameas_from_anon_propagates_to_named() {
    let o = onto(
        "Declaration(Class(:A)) Declaration(NamedIndividual(:a))\n\
         ClassAssertion(:A _:x) SameIndividual(:a _:x)",
    );
    let members = instances_of(&o, "http://e#A").expect("instances_of");
    assert!(
        members.iter().any(|m| m == "http://e#a"),
        "SameAs(:a,_:x) must make :a ∈ A: {members:?}"
    );
    assert!(
        members.iter().all(|m| !m.starts_with("urn:rustdl-anon:")),
        "anon still filtered"
    );
}
