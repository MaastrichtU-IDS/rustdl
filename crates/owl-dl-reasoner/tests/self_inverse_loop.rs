//! #437: the main tableau read a `Self` self-loop in one polarity only.
//!
//! `apply_self_restriction` stores every self-loop as a forward `r(x, x)`, whatever the
//! polarity of the `Self` that asked for it, but checked for an existing loop with
//! `edge_satisfies(Named(r), wanted)`. For `Self(r⁻)` that is
//! `edge_satisfies(Named(r), Inverse(r))`, which is false without a declared inverse.
//! So the rule never saw the loop it had just added, and added another on every sweep
//! (`add_edge` does not deduplicate). Saturation never stabilised, ran out of
//! iterations, and ended in `NoVerdict`: `consistent` and `sat` failed on a 5-axiom
//! `TBox` (reproducer A below). The negative half used the same test, so `Self(r)` with
//! `¬Self(r⁻)` (or a declared inverse, or a sub-role) was not a clash, and `sat` called
//! an unsatisfiable class satisfiable. A loop is `r(x, x)` and `r⁻(x, x)` at once, so
//! the fix reads it in both polarities.
//!
//! Every verdict here was checked with HermiT, and with Konclude where Konclude reports
//! the relation. Konclude misses the `C ⊑ D` in
//! `self_inverse_with_forall_gives_the_subsumption` (an under-report; HermiT derives
//! it, and it follows from `r⁻(x, x) ⟹ r(x, x)`).
//!
//! The `sat`/`consistent`/`subclass` tests run the main tableau, which is where the bug
//! was. `classify_agrees_on_the_clash_shapes` runs the wedge instead: the wedge was
//! already right on every shape, so that test is a control, not coverage for the fix.

#![allow(clippy::unwrap_used)]
#![allow(clippy::doc_markdown)]

use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::{classify, is_class_satisfiable, is_consistent, is_subclass_of};

const EX: &str = "http://ex.org/";

fn parse(src: &str) -> SetOntology<RcStr> {
    let mut cur = std::io::Cursor::new(src.to_owned());
    horned_owl::io::ofn::reader::read(&mut cur, horned_owl::io::ParserConfiguration::default())
        .expect("parse")
        .0
}

/// An ontology over `:C :D :E` and `:r :s :p` with `body` as its axioms.
fn onto(body: &str) -> SetOntology<RcStr> {
    parse(&format!(
        "Prefix(:=<{EX}>)\nPrefix(owl:=<http://www.w3.org/2002/07/owl#>)\n\
         Ontology(<{EX}t>\n\
         Declaration(Class(:C)) Declaration(Class(:D)) Declaration(Class(:E))\n\
         Declaration(ObjectProperty(:r)) Declaration(ObjectProperty(:s))\n\
         Declaration(ObjectProperty(:p))\n{body}\n)\n"
    ))
}

fn sat(o: &SetOntology<RcStr>, class: &str) -> bool {
    is_class_satisfiable(o, &format!("{EX}{class}")).expect("sat reaches a verdict")
}

/// Reproducer A from the issue (ddmin-shrunk from #363 case 1283). Konclude and HermiT:
/// consistent, `C1` unsatisfiable, `C2`, `C4`, `C5` equivalent to `owl:Thing`.
const REPRO_A: &str = "Prefix(:=<http://gen.rustdl/o#>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Ontology(<http://gen.rustdl/o>
Declaration(Class(:C1)) Declaration(Class(:C2)) Declaration(Class(:C4)) Declaration(Class(:C5))
Declaration(ObjectProperty(:r0)) Declaration(ObjectProperty(:r1))
SubClassOf(ObjectSomeValuesFrom(:r1 owl:Thing) :C4)
DisjointUnion(:C2 :C1 :C4)
SubClassOf(:C5 :C4)
EquivalentClasses(:C5 ObjectAllValuesFrom(:r1 ObjectUnionOf(:C5 :C2)))
SubClassOf(:C2 ObjectSomeValuesFrom(:r0 ObjectHasSelf(ObjectInverseOf(:r1))))
";

#[test]
fn reproducer_a_is_consistent_and_every_class_gets_a_verdict() {
    let o = parse(&format!("{REPRO_A})\n"));
    assert!(is_consistent(&o).expect("consistent reaches a verdict"));
    let sat_of = |c: &str| {
        is_class_satisfiable(&o, &format!("http://gen.rustdl/o#{c}")).expect("sat verdict")
    };
    assert!(!sat_of("C1"), "C1 is unsatisfiable");
    for c in ["C2", "C4", "C5"] {
        assert!(sat_of(c), "{c} is satisfiable");
    }
}

/// Reproducer B: A plus `C1 ≡ ∃r0.C4`. Konclude and HermiT: inconsistent. Before the
/// fix `consistent` failed with `NoVerdict`.
#[test]
fn reproducer_b_is_inconsistent() {
    let o = parse(&format!(
        "{REPRO_A}EquivalentClasses(:C1 ObjectSomeValuesFrom(:r0 :C4))\n)\n"
    ));
    assert!(!is_consistent(&o).expect("consistent reaches a verdict"));
}

/// The smallest livelock: `⊤ ⊑ ∃r⁻.Self` puts the loop on the root of the consistency
/// test. `C ⊑ ∃r⁻.Self` alone would not: with no individual, nothing places `C` on a
/// node, so `consistent` was already right on it, and only `sat C` exercised the bug.
#[test]
fn a_self_on_an_inverse_role_terminates() {
    let top = onto("SubClassOf(owl:Thing ObjectHasSelf(ObjectInverseOf(:r)))");
    assert!(is_consistent(&top).expect("consistent reaches a verdict"));
    assert!(sat(&top, "C"));
    let local = onto("SubClassOf(:C ObjectHasSelf(ObjectInverseOf(:r)))");
    assert!(sat(&local, "C"));
}

/// The negative half: a forward loop must clash with `¬Self(r⁻)`. The mirror (loop
/// asked for by `Self(r⁻)`, forbidden by `¬Self(r)`) was already right, because the
/// loop is stored forward; it stays as a control.
#[test]
fn self_and_not_self_of_the_inverse_clash() {
    let fwd = onto(
        "SubClassOf(:C ObjectIntersectionOf(ObjectHasSelf(:r) \
         ObjectComplementOf(ObjectHasSelf(ObjectInverseOf(:r)))))",
    );
    assert!(!sat(&fwd, "C"), "Self(r) ⊓ ¬Self(r⁻) is unsatisfiable");
    let inv = onto(
        "SubClassOf(:C ObjectIntersectionOf(ObjectHasSelf(ObjectInverseOf(:r)) \
         ObjectComplementOf(ObjectHasSelf(:r))))",
    );
    assert!(!sat(&inv, "C"), "Self(r⁻) ⊓ ¬Self(r) is unsatisfiable");
}

/// A declared inverse and a sub-role must count through the loop's INVERSE view. These
/// tell the fix apart from a check that only compares role ids in either polarity.
#[test]
fn a_declared_inverse_or_a_sub_role_counts_through_the_inverse_view() {
    // `s ≡ r⁻`, so a `r`-loop is an `s`-loop.
    let declared = onto(
        "InverseObjectProperties(:r :s) \
         SubClassOf(:C ObjectIntersectionOf(ObjectHasSelf(:r) \
         ObjectComplementOf(ObjectHasSelf(:s))))",
    );
    assert!(!sat(&declared, "C"), "Self(r) ⊓ ¬Self(s) with s ≡ r⁻");
    // `r ⊑ s`, so a `r`-loop is an `s⁻`-loop.
    let sub = onto(
        "SubObjectPropertyOf(:r :s) \
         SubClassOf(:C ObjectIntersectionOf(ObjectHasSelf(:r) \
         ObjectComplementOf(ObjectHasSelf(ObjectInverseOf(:s)))))",
    );
    assert!(!sat(&sub, "C"), "Self(r) ⊓ ¬Self(s⁻) with r ⊑ s");
}

/// FP guards: a loop must not satisfy a `Self` on an unrelated role, nor on a SUPER-role
/// read the wrong way round (`s ⊑ r` does not make a `r`-loop an `s`-loop).
#[test]
fn an_unrelated_or_super_role_does_not_clash() {
    let unrelated = onto(
        "SubClassOf(:C ObjectIntersectionOf(ObjectHasSelf(:r) \
         ObjectComplementOf(ObjectHasSelf(ObjectInverseOf(:s)))))",
    );
    assert!(sat(&unrelated, "C"), "unrelated s");
    let wrong_way = onto(
        "SubObjectPropertyOf(:s :r) \
         SubClassOf(:C ObjectIntersectionOf(ObjectHasSelf(:r) \
         ObjectComplementOf(ObjectHasSelf(ObjectInverseOf(:s)))))",
    );
    assert!(sat(&wrong_way, "C"), "s ⊑ r is the wrong direction");
}

/// The negative half at a successor, on a loop another class asked for: `D ⊑ ∃r.Self`
/// gives the `p`-successor a forward loop, and `∀p.¬∃r⁻.Self` forbids it. The mirror
/// (`D ⊑ ∃r⁻.Self`, `∀p.¬∃r.Self`) was already unsat, and its `D` hit the livelock.
#[test]
fn the_clash_is_found_at_a_successor() {
    let o = onto(
        "SubClassOf(:C ObjectIntersectionOf(ObjectSomeValuesFrom(:p :D) \
         ObjectAllValuesFrom(:p ObjectComplementOf(ObjectHasSelf(ObjectInverseOf(:r)))))) \
         SubClassOf(:D ObjectHasSelf(:r))",
    );
    assert!(!sat(&o, "C"));
    assert!(sat(&o, "D"));
    let mirror = onto(
        "SubClassOf(:C ObjectIntersectionOf(ObjectSomeValuesFrom(:p :D) \
         ObjectAllValuesFrom(:p ObjectComplementOf(ObjectHasSelf(:r))))) \
         SubClassOf(:D ObjectHasSelf(ObjectInverseOf(:r)))",
    );
    assert!(!sat(&mirror, "C"));
    assert!(sat(&mirror, "D"));
}

/// The loop is also seen by `∀`: `r⁻(x, x)` is `r(x, x)`, so `∀r.D` applies to `x`.
#[test]
fn self_inverse_with_forall_gives_the_subsumption() {
    let o = onto(
        "SubClassOf(:C ObjectIntersectionOf(ObjectHasSelf(ObjectInverseOf(:r)) \
         ObjectAllValuesFrom(:r :D)))",
    );
    assert!(sat(&o, "C"));
    assert!(is_subclass_of(&o, &format!("{EX}C"), &format!("{EX}D")).expect("verdict"));
}

/// Wedge control: `classify` was already right on these. Asserted so a regression in the
/// wedge's own reading of a self-loop would show up next to the tableau tests.
#[test]
fn classify_agrees_on_the_clash_shapes() {
    for body in [
        "SubClassOf(:C ObjectIntersectionOf(ObjectHasSelf(:r) \
         ObjectComplementOf(ObjectHasSelf(ObjectInverseOf(:r)))))",
        "InverseObjectProperties(:r :s) \
         SubClassOf(:C ObjectIntersectionOf(ObjectHasSelf(:r) \
         ObjectComplementOf(ObjectHasSelf(:s))))",
        "SubClassOf(:C ObjectIntersectionOf(ObjectSomeValuesFrom(:p :D) \
         ObjectAllValuesFrom(:p ObjectComplementOf(ObjectHasSelf(ObjectInverseOf(:r)))))) \
         SubClassOf(:D ObjectHasSelf(:r))",
    ] {
        let c = classify(&onto(body)).expect("classify");
        assert!(
            c.unsatisfiable_classes().iter().any(|u| u.ends_with("/C")),
            "{body}"
        );
    }
}
