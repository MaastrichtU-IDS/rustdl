//! #149, the wide form: `InverseFunctionalObjectProperty` was inert on any
//! `ABox`-free ontology — `inv_func_merge_consumable` admitted the derived
//! `≤1 r⁻` GCI only when the role had an `ObjectPropertyAssertion`, so a pure
//! `TBox` got no constraint and a wrong `satisfiable` with `dropped: {}`.
//!
//! The TBox-aware admission (this issue's explicit ask) reasons from what the
//! GCI can DO: `∃r⁻.⊤ ⊑ ≤1 r⁻` fires a merge only on a node with two
//! `r⁻`-successors, so admit exactly the ontologies whose `TBox` can construct
//! that — two `∃r⁻`/`≥n r⁻` generators, one `≥n r⁻` with n ≥ 2, or an
//! `∃r.{a}` (nominal targets are shared). The bar it must clear is recorded in
//! the gate's doc: the three ontologies whose 19–47× regression created the
//! gate (`ore_ont_9662`/`7532`/`9786`) must stay excluded, and they do — their
//! IF roles have no generators (verified per-role; `9662`'s three
//! inverse-existentials are on `bearer_of`, a non-IF role).
//!
//! The FRAGMENT is the cheap observable for admission: the derived `Max` kicks
//! an otherwise-EL ontology off the pure-EL fast path, so `pure_el_mode` tells
//! us whether the GCI was emitted without groveling through internals.

#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use std::io::Cursor;

fn parse(ofn: &str) -> SetOntology<RcStr> {
    let (o, _) = read_ofn(
        &mut Cursor::new(ofn.to_string()),
        ParserConfiguration::default(),
    )
    .expect("parse ofn");
    o
}

fn sat(ofn: &str, class: &str) -> bool {
    owl_dl_reasoner::is_class_satisfiable(&parse(ofn), &format!("http://ex.org/{class}"))
        .expect("satisfiability query")
}

fn ont(body: &str) -> String {
    format!(
        "Prefix(:=<http://ex.org/>)\nOntology(<http://ex.org/w>\n\
Declaration(Class(:A)) Declaration(Class(:C)) Declaration(Class(:D))\n\
Declaration(ObjectProperty(:p))\n\
InverseFunctionalObjectProperty(:p)\n{body}\n)\n"
    )
}

#[test]
fn two_inverse_existentials_admit_the_gci_and_clash() {
    // THE WIDE-FORM FIX, on the classify surface (the wedge derives the merge
    // clash in milliseconds; the deadline-free tableau twin below takes ~220 s
    // in the debug profile and is #[ignore]d with that reason). `A` has two
    // p⁻-successors in disjoint classes; `InverseFunctional(p)` merges them.
    // Pure TBox — before this fix, satisfiable with `dropped: {}`.
    let ofn = ont("DisjointClasses(:C :D)\n\
         EquivalentClasses(:A ObjectIntersectionOf(\
         ObjectSomeValuesFrom(ObjectInverseOf(:p) :C) \
         ObjectSomeValuesFrom(ObjectInverseOf(:p) :D)))");
    let h = owl_dl_reasoner::classify(&parse(&ofn)).expect("classify");
    assert!(
        h.unsatisfiable_classes().contains(&"http://ex.org/A"),
        "InverseFunctional(p) bounds p⁻-successors: classify must report A unsat"
    );
}

#[test]
#[ignore = "~220 s in the debug profile (deadline-free tableau); the classify canary above covers the default path in ms"]
fn the_tableau_satisfiability_surface_agrees() {
    let ofn = ont("DisjointClasses(:C :D)\n\
         EquivalentClasses(:A ObjectIntersectionOf(\
         ObjectSomeValuesFrom(ObjectInverseOf(:p) :C) \
         ObjectSomeValuesFrom(ObjectInverseOf(:p) :D)))");
    assert!(
        !sat(&ofn, "A"),
        "is_class_satisfiable must agree with classify: A unsat"
    );
}

#[test]
fn forward_only_usage_is_not_admitted_and_derives_nothing() {
    // FP GUARD + the blocker shape in miniature. `IF(p)` says nothing about
    // p-SUCCESSOR multiplicity, and forward-only usage is exactly what
    // `ore_ont_9662`/`7532`/`9786` do 8 roles × hundreds of existentials over.
    let ofn = ont("DisjointClasses(:C :D)\n\
         EquivalentClasses(:A ObjectIntersectionOf(\
         ObjectSomeValuesFrom(:p :C) ObjectSomeValuesFrom(:p :D)))");
    assert!(
        sat(&ofn, "A"),
        "IF(p) does not merge p-successors — unsat here is an FP"
    );
}

#[test]
fn the_blocker_shape_keeps_the_fast_path() {
    // The COST half of the gate, pinned via routing: an EL ontology using its
    // IF role forward-only must stay `pure_el_mode` — i.e. the GCI was NOT
    // emitted. This is the observable that would have caught an over-broad
    // predicate ("the role appears in a class expression"), which the gate's
    // doc explicitly names as not narrow enough.
    let ofn = ont("SubClassOf(:A ObjectSomeValuesFrom(:p :C))");
    let h = owl_dl_reasoner::classify(&parse(&ofn)).expect("classify");
    assert!(
        h.stats().pure_el_mode,
        "forward-only IF usage must not emit the ≤1 GCI (it kicks EL ontologies \
         off the fast path for a merge that can never fire)"
    );
}

#[test]
fn a_nominal_target_admits_the_gci() {
    // `∃p.{a}` shares its target, so two nodes carrying it give `a` two
    // p-predecessors — merge material even with ONE generator. Admission is
    // observed via routing (nominals leave EL anyway, so use the axiom count
    // observable instead: the clash itself). Konclude semantics: x ∈ ∃p.{a}
    // and y ∈ ∃p.{a} force x = y under IF(p); with x ⊑ C, y ⊑ D, C/D disjoint,
    // anything in both C-side and D-side collapses. Directly: A ≡ ∃p.{a} ⊓ C
    // ⊓ D-side is overkill — assert the simplest consequence: A ≡ ∃p.{a} with
    // `A ⊑ C`, `B ≡ ∃p.{a}` with `B ⊑ D`, C/D disjoint ⟹ A ⊓ B unsat is
    // trivial; the MERGE consequence is A ⊑ B (any A-member has p→a, so it is
    // the unique p-predecessor of a, as is any B-member ⟹ same individual ⟹
    // every A is a B). Konclude derives A ≡ B on this shape.
    let ofn = "Prefix(:=<http://ex.org/>)\nOntology(<http://ex.org/n>\n\
Declaration(Class(:A)) Declaration(Class(:B))\n\
Declaration(ObjectProperty(:p)) Declaration(NamedIndividual(:a))\n\
InverseFunctionalObjectProperty(:p)\n\
EquivalentClasses(:A ObjectHasValue(:p :a))\n\
EquivalentClasses(:B ObjectHasValue(:p :a))\n)\n";
    // A and B are syntactically identical definitions, so equivalence holds for
    // boring reasons; what the admission buys is the MERGE machinery being
    // armed at all. Assert the routing observable: the GCI must be emitted.
    let h = owl_dl_reasoner::classify(&parse(ofn)).expect("classify");
    assert!(
        !h.stats().pure_el_mode,
        "a nominal-targeted IF role is merge material — the GCI must be emitted \
         (routing off the pure-EL path is its cheap observable)"
    );
}

#[test]
fn min_two_on_the_inverse_role_clashes_on_the_satisfiability_surface() {
    // `A ⊑ ≥2 p⁻` with `IF(p)` (i.e. `≤1 p⁻`) is a direct arithmetic clash. The
    // admission predicate emits the GCI (a single `≥n, n≥2` generator) and
    // `is_class_satisfiable` derives the clash in milliseconds. (An earlier
    // draft of this test pinned it as an "engine gap" from a CLASSIFY-surface
    // probe — a wrong-surface control, the trap this repo's record warns about
    // in both directions.)
    let ofn = ont("SubClassOf(:A ObjectMinCardinality(2 ObjectInverseOf(:p)))");
    assert!(!sat(&ofn, "A"), "≥2 p⁻ ∧ ≤1 p⁻ clashes: A is unsatisfiable");
}

#[test]
fn min_two_on_the_inverse_role_clashes_on_the_classify_surface() {
    // FLIPPED by #190 gap 1 (was `…_is_still_missed_by_classify`, a pinned
    // known gap). The wedge never generated an inverse-polarity `≥n`, so it
    // answered `Sat` here and `trust_sat` accepted it — the #66/#76
    // trusted-Sat family. It now generates the two `p⁻`-successors and the
    // `≤1 p⁻` clash follows.
    let ofn = ont("SubClassOf(:A ObjectMinCardinality(2 ObjectInverseOf(:p)))");
    let h = owl_dl_reasoner::classify(&parse(&ofn)).expect("classify");
    assert!(
        h.unsatisfiable_classes().contains(&"http://ex.org/A"),
        "≥2 p⁻ ∧ ≤1 p⁻ clashes: classify must report A unsatisfiable"
    );
}

/// #207: `InverseFunctional(p)` bounds `p⁻`, so a `≥2` over a role that is
/// `p⁻` only through an `InverseObjectProperties` axiom must be admitted —
/// and one that is `p` itself through the same axiom must not. Verdicts are
/// `HermiT`'s and Konclude's (both agree on all three).
fn with_q(inverse_axiom: &str, min: &str) -> String {
    ont(&format!(
        "Declaration(ObjectProperty(:q)) {inverse_axiom} \
         SubClassOf(:A ObjectMinCardinality(2 {min} :D))"
    ))
}

fn classify_unsat(ofn: &str) -> bool {
    owl_dl_reasoner::classify(&parse(ofn))
        .expect("classify")
        .unsatisfiable_classes()
        .contains(&"http://ex.org/A")
}

#[test]
fn a_min_two_through_the_declared_inverse_clashes_either_argument_order() {
    for inv in [
        "InverseObjectProperties(:p :q)",
        "InverseObjectProperties(:q :p)",
    ] {
        assert!(
            classify_unsat(&with_q(inv, ":q")),
            "{inv}: q = p⁻, ≥2 q ∧ ≤1 p⁻ clashes"
        );
    }
}

#[test]
fn the_declared_inverse_is_resolved_with_its_polarity() {
    // FP guard: with `q = p⁻`, `≥2 q⁻` is two p-SUCCESSORS, which `IF(p)` does
    // not bound. (The `InverseObjectProperties(ObjectInverseOf(:p) :q)`
    // spelling the gate also handles is rejected by the OFN parser.)
    let inv = "InverseObjectProperties(:p :q)";
    assert!(
        !classify_unsat(&with_q(inv, "ObjectInverseOf(:q)")),
        "q⁻ = p: unsat here is an FP"
    );
}

/// A self-inverse role is its own `p⁻`, so `≥2 p` under `IF(p)` clashes —
/// whether the symmetry is spelled `InverseObjectProperties(:p :p)` or
/// `SymmetricObjectProperty(:p)`. The second spelling was missed by the
/// admission gate after #207 fixed the first (review of #208).
#[test]
fn a_min_two_on_a_self_inverse_role_clashes_in_both_spellings() {
    for decl in [
        "InverseObjectProperties(:p :p)",
        "SymmetricObjectProperty(:p)",
    ] {
        let ofn = ont(&format!(
            "{decl} SubClassOf(:A ObjectMinCardinality(2 :p :D))"
        ));
        assert!(
            classify_unsat(&ofn),
            "{decl}: p = p⁻, so ≥2 p ∧ ≤1 p⁻ clashes"
        );
    }
}

/// #209: a `≥2` on any role provably below `p⁻` clashes with `IF(p)`, however
/// the relation is spelled; one equal to or above `p` (or above `p⁻`) does
/// not. Every verdict is `HermiT`'s and Konclude's.
fn with_roles(axioms: &str) -> String {
    ont(&format!(
        "Declaration(ObjectProperty(:q)) Declaration(ObjectProperty(:s)) \
         Declaration(ObjectProperty(:t)) {axioms}"
    ))
}

#[test]
fn a_min_two_below_the_inverse_through_role_axioms_clashes() {
    for axioms in [
        "InverseObjectProperties(:p :q) InverseObjectProperties(:q :s) \
         SubClassOf(:A ObjectMinCardinality(2 ObjectInverseOf(:s) :D))",
        "InverseObjectProperties(:p :q) InverseObjectProperties(:q :s) \
         InverseObjectProperties(:s :t) SubClassOf(:A ObjectMinCardinality(2 :t :D))",
        "EquivalentObjectProperties(:p :s) \
         SubClassOf(:A ObjectMinCardinality(2 ObjectInverseOf(:s) :D))",
        "InverseObjectProperties(:p :q) EquivalentObjectProperties(:q :s) \
         SubClassOf(:A ObjectMinCardinality(2 :s :D))",
        "InverseObjectProperties(:p :q) SubObjectPropertyOf(:s :q) \
         SubClassOf(:A ObjectMinCardinality(2 :s :D))",
        "SubObjectPropertyOf(:s :p) SubClassOf(:A ObjectMinCardinality(2 ObjectInverseOf(:s) :D))",
    ] {
        assert!(
            classify_unsat(&with_roles(axioms)),
            "should clash: {axioms}"
        );
    }
}

#[test]
fn a_min_two_not_below_the_inverse_does_not_clash() {
    for axioms in [
        // a SUPER-role of p⁻
        "InverseObjectProperties(:p :q) SubObjectPropertyOf(:q :s) \
         SubClassOf(:A ObjectMinCardinality(2 :s :D))",
        // s = p
        "EquivalentObjectProperties(:p :s) SubClassOf(:A ObjectMinCardinality(2 :s :D))",
        // s = q⁻ = p
        "InverseObjectProperties(:p :q) InverseObjectProperties(:q :s) \
         SubClassOf(:A ObjectMinCardinality(2 :s :D))",
        // s ⊑ p: s-successors are p-successors, which IF(p) does not bound
        "SubObjectPropertyOf(:s :p) SubClassOf(:A ObjectMinCardinality(2 :s :D))",
    ] {
        assert!(
            !classify_unsat(&with_roles(axioms)),
            "unsat here is an FP: {axioms}"
        );
    }
}

/// KNOWN GAP (#211), pinned: #209's gate emits the `≤1 p⁻` GCI here and
/// `classify` derives the clash (above), but the per-query surface does not
/// apply `≤1 p⁻` to `s`-successors when `s ⊑ p⁻` holds only through the role
/// hierarchy. `HermiT` and Konclude both call `A` unsatisfiable. FLIP this to
/// `!sat(..)` when #211 closes.
#[test]
fn a_min_two_on_a_sub_role_of_the_inverse_is_still_missed_by_sat() {
    let ofn = with_roles(
        "SubObjectPropertyOf(ObjectInverseOf(:s) :p) SubClassOf(:A ObjectMinCardinality(2 :s :D))",
    );
    assert!(classify_unsat(&ofn), "classify must still derive the clash");
    assert!(
        sat(&ofn, "A"),
        "#211 closed: flip this test to assert A unsatisfiable on the sat surface"
    );
}
