//! #534: `DisjointObjectProperties` with an inverse operand on the per-query surface.
//!
//! `Disjoint(r, s⁻)` forbids `r(a,b) ∧ s(b,a)`. The disjoint-role pairs the main tableau
//! and `abox_check` P9 check had no polarity, so every pair with an inverse operand was
//! skipped. `classify` found these clashes through the wedge, whose clauses were
//! signed, while `sat` / `subclass` (decided on the main tableau) answered `sat` / `no`,
//! and on an inconsistent `ABox` the per-query guard said nothing. The pairs are now
//! signed, with the first operand forward.
//!
//! Every verdict is adjudicated by `HermiT`; Konclude agrees wherever it reports, and
//! under-reports on the inverse-form shapes marked below (as recorded on the issue).
//! Negatives first: each positive has an FP guard of the same shape that a
//! polarity-blind check would get wrong.
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::{ReasonError, classify, is_class_satisfiable, is_subclass_of};

const EX: &str = "http://ex/#";
const NOTHING: &str = "http://www.w3.org/2002/07/owl#Nothing";

fn onto(body: &str) -> SetOntology<RcStr> {
    let src = format!(
        "Prefix(:=<{EX}>)\nPrefix(owl:=<http://www.w3.org/2002/07/owl#>)\n\
         Ontology(<http://ex/o>\n\
         Declaration(Class(:C)) Declaration(Class(:D)) Declaration(Class(:Q))\n\
         Declaration(ObjectProperty(:r)) Declaration(ObjectProperty(:s))\n\
         Declaration(ObjectProperty(:t)) Declaration(ObjectProperty(:q))\n\
         Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:a2))\n\
         Declaration(NamedIndividual(:b))\n{body}\n)\n"
    );
    read_ofn(&mut Cursor::new(src), ParserConfiguration::default())
        .unwrap()
        .0
}

fn sat(o: &SetOntology<RcStr>, class: &str) -> bool {
    is_class_satisfiable(o, &format!("{EX}{class}")).expect("sat reaches a verdict")
}

/// The per-query answer, `subclass C owl:Nothing`, and classify must all agree.
fn assert_unsat_everywhere(body: &str, class: &str) {
    let o = onto(body);
    assert!(!sat(&o, class), "sat {class}: {body}");
    assert!(
        is_subclass_of(&o, &format!("{EX}{class}"), NOTHING).expect("verdict"),
        "subclass {class} owl:Nothing: {body}"
    );
    let c = classify(&o).expect("classify");
    assert!(
        c.unsatisfiable_classes()
            .iter()
            .any(|u| u.ends_with(&format!("#{class}"))),
        "classify {class}: {body}"
    );
}

fn assert_sat_everywhere(body: &str, class: &str) {
    let o = onto(body);
    assert!(sat(&o, class), "sat {class}: {body}");
    let c = classify(&o).expect("classify");
    assert!(
        !c.unsatisfiable_classes()
            .iter()
            .any(|u| u.ends_with(&format!("#{class}"))),
        "classify {class}: {body}"
    );
}

/// On an inconsistent ontology the named class query errors (#428).
fn assert_inconsistent_per_query(body: &str) {
    let r = is_class_satisfiable(&onto(body), &format!("{EX}C"));
    assert!(matches!(r, Err(ReasonError::Inconsistent)), "{body}: {r:?}");
}

// ── FP guards: a polarity-blind check gets each of these wrong ──────────────────

/// `Disjoint(r, s⁻)` alone does not restrict an `r`-successor (no `s` edge back).
#[test]
fn a_mixed_pair_alone_leaves_c_satisfiable() {
    assert_sat_everywhere(
        "DisjointObjectProperties(:r ObjectInverseOf(:s)) \
         SubClassOf(:C ObjectSomeValuesFrom(:r :Q))",
        "C",
    );
}

/// `r(a,b)` with `s(a,b)` is the SAME direction: `Disjoint(r, s⁻)` forbids `s(b,a)`.
#[test]
fn a_mixed_pair_with_same_direction_assertions_is_consistent() {
    let o = onto(
        "DisjointObjectProperties(:r ObjectInverseOf(:s)) \
         ObjectPropertyAssertion(:r :a :b) ObjectPropertyAssertion(:s :a :b)",
    );
    assert!(sat(&o, "C"));
    assert!(sat(&o, "Q"));
}

/// The merge shape with a forward pair: the merged successor carries `s(x,y)` and
/// `r(y,x)`, which `Disjoint(r, s)` does not forbid.
#[test]
fn a_forward_pair_does_not_clash_on_a_reversed_merge() {
    let body = "SubObjectPropertyOf(:s :t) SubObjectPropertyOf(ObjectInverseOf(:r) :t) \
                DisjointObjectProperties(:r :s) \
                SubClassOf(:C ObjectIntersectionOf(ObjectSomeValuesFrom(:s :Q) \
                ObjectSomeValuesFrom(ObjectInverseOf(:r) :Q) ObjectMaxCardinality(1 :t)))";
    assert_sat_everywhere(body, "C");
}

/// `Disjoint(r, r⁻)` (asymmetry) alone leaves an `r`-successor satisfiable.
#[test]
fn asymmetry_form_alone_leaves_c_satisfiable() {
    assert_sat_everywhere(
        "DisjointObjectProperties(:r ObjectInverseOf(:r)) \
         SubClassOf(:C ObjectSomeValuesFrom(:r :Q))",
        "C",
    );
}

// ── TBox: the per-query surface now finds what classify found ────────────────────

/// `Disjoint(s⁻, r⁻)` is `Disjoint(s, r)`; `r ⊑ s` empties `r`.
#[test]
fn both_operands_inverse() {
    assert_unsat_everywhere(
        "SubObjectPropertyOf(:r :s) SubClassOf(:C ObjectSomeValuesFrom(:r :Q)) \
         DisjointObjectProperties(ObjectInverseOf(:s) ObjectInverseOf(:r))",
        "C",
    );
}

/// `Inv(r, s)` makes `s⁻ = r`, so `Disjoint(r, s⁻)` empties `r`.
#[test]
fn a_mixed_pair_through_a_declared_inverse() {
    assert_unsat_everywhere(
        "InverseObjectProperties(:r :s) DisjointObjectProperties(:r ObjectInverseOf(:s)) \
         SubClassOf(:C ObjectSomeValuesFrom(:r :Q))",
        "C",
    );
}

/// `r ⊑ s⁻` with `Disjoint(r, s⁻)` empties `r`.
#[test]
fn a_mixed_pair_through_a_sub_role() {
    assert_unsat_everywhere(
        "SubObjectPropertyOf(:r ObjectInverseOf(:s)) \
         DisjointObjectProperties(:r ObjectInverseOf(:s)) \
         SubClassOf(:C ObjectSomeValuesFrom(:r :Q))",
        "C",
    );
}

/// The `≤1 t` merge puts `s(x,y)` and `r(y,x)` on one pair, which `Disjoint(r, s⁻)`
/// forbids. Both operand orders: they are detected at opposite endpoints.
#[test]
fn a_merge_creates_the_reversed_pair() {
    let base = "SubObjectPropertyOf(:s :t) SubObjectPropertyOf(ObjectInverseOf(:r) :t) \
                SubClassOf(:C ObjectIntersectionOf(ObjectSomeValuesFrom(:s :Q) \
                ObjectSomeValuesFrom(ObjectInverseOf(:r) :Q) ObjectMaxCardinality(1 :t))) \
                SubClassOf(:D ObjectIntersectionOf(ObjectSomeValuesFrom(:s :Q) \
                ObjectSomeValuesFrom(ObjectInverseOf(:r) :Q)))";
    for disjoint in [
        "DisjointObjectProperties(:r ObjectInverseOf(:s))",
        "DisjointObjectProperties(ObjectInverseOf(:s) :r)",
    ] {
        let body = format!("{base} {disjoint}");
        assert_unsat_everywhere(&body, "C");
        // Without the merge there is no shared pair.
        assert_sat_everywhere(&body, "D");
    }
}

/// `Disjoint(r, r⁻)` forbids `r(x,x)`, so a self-loop is a clash (Konclude reports
/// only `C`; `HermiT` reports `C` and `Q`).
#[test]
fn asymmetry_form_rejects_a_self_loop() {
    let body = "DisjointObjectProperties(:r ObjectInverseOf(:r)) \
                SubClassOf(:Q ObjectHasSelf(:r)) \
                SubClassOf(:C ObjectSomeValuesFrom(:r ObjectHasSelf(:r)))";
    assert_unsat_everywhere(body, "Q");
    assert_unsat_everywhere(body, "C");
}

// ── ABox: the per-query surface now reports the inconsistency ────────────────────

/// `r(a,b)`, `s(b,a)` (Konclude under-reports this one).
#[test]
fn reversed_assertions_are_inconsistent() {
    assert_inconsistent_per_query(
        "DisjointObjectProperties(:r ObjectInverseOf(:s)) \
         ObjectPropertyAssertion(:r :a :b) ObjectPropertyAssertion(:s :b :a)",
    );
    // The issue's other spelling: `Disjoint(r⁻, s)` is `Disjoint(r, s⁻)`.
    assert_inconsistent_per_query(
        "DisjointObjectProperties(ObjectInverseOf(:r) :s) \
         ObjectPropertyAssertion(:r :b :a) ObjectPropertyAssertion(:s :a :b) \
         SameIndividual(:a2 :b)",
    );
}

/// The clash needs the role hierarchy (`q ⊑ s`), so P9's verbatim match is silent and
/// the tableau decides (Konclude under-reports this one).
#[test]
fn a_sub_role_assertion_is_inconsistent() {
    assert_inconsistent_per_query(
        "SubObjectPropertyOf(:q :s) DisjointObjectProperties(:r ObjectInverseOf(:s)) \
         ObjectPropertyAssertion(:r :a :b) ObjectPropertyAssertion(:q :b :a) \
         ClassAssertion(:C :a)",
    );
}

/// The clash needs a merge (`a = a2`), so P9's verbatim match is silent again.
#[test]
fn a_merged_individual_is_inconsistent() {
    assert_inconsistent_per_query(
        "DisjointObjectProperties(:r ObjectInverseOf(:s)) \
         ObjectPropertyAssertion(:r :a :b) ObjectPropertyAssertion(:s :b :a2) \
         SameIndividual(:a :a2)",
    );
}
