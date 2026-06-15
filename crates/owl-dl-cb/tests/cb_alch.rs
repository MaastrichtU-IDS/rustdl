//! Integration canaries for the consequence-based ALCH engine.
//!
//! These tests exercise capabilities the EL saturator **cannot** handle
//! (disjunction, `∀`+`∃`+`¬` clashes, reasoning by cases) plus a pure-EL
//! sanity check and a non-subsumption FP guard.
//!
//! **Status:** RED pending Tasks A (normalize) + B (engine) integration.
//! The tests compile against the frozen `owl_dl_cb::classify` API (Task 0)
//! but panic at runtime via `todo!()` until A+B land. That is the expected
//! pre-integration state; they encode the contract.
//!
//! Pipeline: OFN text → horned-owl `SetOntology` → `convert_ontology` →
//! `owl_dl_cb::classify` — no intermediate passes between convert and classify.
//!
//! Run: `cargo test -p owl-dl-cb --test cb_alch`.

#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_cb::{CbHierarchy, CbOutcome};
use owl_dl_core::convert::convert_ontology;
use std::io::Cursor;

const PFX: &str = "Prefix(:=<http://t/>)\nPrefix(owl:=<http://www.w3.org/2002/07/owl#>)\n";

// ── helpers that carry both the hierarchy AND the internal ontology ────────────

struct Classified {
    hierarchy: CbHierarchy,
    internal: owl_dl_core::ontology::InternalOntology,
}

fn classify_alch_with_internal(body: &str) -> Classified {
    let src = format!("{PFX}Ontology(<http://t/x>\n{body}\n)\n");
    let mut reader = Cursor::new(src);
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut reader, ParserConfiguration::default()).expect("OFN parse error");
    let internal = convert_ontology(&onto).expect("convert_ontology error");
    let hierarchy = match owl_dl_cb::classify(&internal) {
        CbOutcome::Classified(h) => h,
        CbOutcome::OutOfFragment(reason) => {
            panic!("unexpected OutOfFragment for pure-ALCH input: {reason}")
        }
    };
    Classified {
        hierarchy,
        internal,
    }
}

// ── helpers for the assertions ─────────────────────────────────────────────────

/// Assert that `sub ⊑ sup` is in the hierarchy (sub IRI, sup IRI).
fn assert_subsumes(c: &Classified, sub_iri: &str, sup_iri: &str) {
    let sub = c
        .internal
        .vocabulary
        .class_id(sub_iri)
        .unwrap_or_else(|| panic!("IRI not in vocabulary: {sub_iri}"));
    let sup = c
        .internal
        .vocabulary
        .class_id(sup_iri)
        .unwrap_or_else(|| panic!("IRI not in vocabulary: {sup_iri}"));
    assert!(
        c.hierarchy.subsumptions.contains(&(sub, sup)),
        "expected {sub_iri} ⊑ {sup_iri} in hierarchy (MISSED)"
    );
}

/// Assert that `sub ⊑ sup` is NOT in the hierarchy (FP guard).
fn assert_not_subsumes(c: &Classified, sub_iri: &str, sup_iri: &str) {
    let sub = c
        .internal
        .vocabulary
        .class_id(sub_iri)
        .unwrap_or_else(|| panic!("IRI not in vocabulary: {sub_iri}"));
    let sup = c
        .internal
        .vocabulary
        .class_id(sup_iri)
        .unwrap_or_else(|| panic!("IRI not in vocabulary: {sup_iri}"));
    assert!(
        !c.hierarchy.subsumptions.contains(&(sub, sup)),
        "spurious {sub_iri} ⊑ {sup_iri} in hierarchy (FALSE POSITIVE)"
    );
}

/// Assert that `cls` is in the unsatisfiable set.
fn assert_unsat(c: &Classified, iri: &str) {
    let id = c
        .internal
        .vocabulary
        .class_id(iri)
        .unwrap_or_else(|| panic!("IRI not in vocabulary: {iri}"));
    assert!(
        c.hierarchy.unsat.contains(&id),
        "expected {iri} ∈ unsat (MISSED)"
    );
}

/// Assert that `cls` is NOT in the unsatisfiable set.
fn assert_sat(c: &Classified, iri: &str) {
    let id = c
        .internal
        .vocabulary
        .class_id(iri)
        .unwrap_or_else(|| panic!("IRI not in vocabulary: {iri}"));
    assert!(
        !c.hierarchy.unsat.contains(&id),
        "spurious {iri} ∈ unsat (FALSE POSITIVE unsatisfiability)"
    );
}

// ══════════════════════════════════════════════════════════════════════════════
// 1. Disjunctive subsumption — the headline case the EL saturator CANNOT handle
//
//   A ⊑ B ⊔ C,  B ⊑ D,  C ⊑ D  ⟹  A ⊑ D
//
//   The CB engine must reason by cases: whichever disjunct holds (B or C),
//   D must hold too, so A ⊑ D is a necessary consequence.
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn disjunctive_subsumption_a_sub_d() {
    let c = classify_alch_with_internal(
        r"    Declaration(Class(:A))
    Declaration(Class(:B))
    Declaration(Class(:C))
    Declaration(Class(:D))
    SubClassOf(:A ObjectUnionOf(:B :C))
    SubClassOf(:B :D)
    SubClassOf(:C :D)",
    );
    // A ⊑ D: the key disjunctive entailment
    assert_subsumes(&c, "http://t/A", "http://t/D");
    // B ⊑ D and C ⊑ D must also be present (direct told subsumptions)
    assert_subsumes(&c, "http://t/B", "http://t/D");
    assert_subsumes(&c, "http://t/C", "http://t/D");
}

// ══════════════════════════════════════════════════════════════════════════════
// 2. ∀ + ∃ + ¬ clash → unsat
//
//   A ⊑ ∀R.B,  A ⊑ ∃R.C,  DisjointClasses(B, C)  ⟹  A ⊑ ⊥
//
//   Any R-successor of an A must be B (from the ∀) and C (from the ∃),
//   but B and C are disjoint — contradiction.
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn forall_exists_disjoint_clash_unsat() {
    let c = classify_alch_with_internal(
        r"    Declaration(Class(:A))
    Declaration(Class(:B))
    Declaration(Class(:C))
    Declaration(ObjectProperty(:R))
    SubClassOf(:A ObjectAllValuesFrom(:R :B))
    SubClassOf(:A ObjectSomeValuesFrom(:R :C))
    DisjointClasses(:B :C)",
    );
    // A is unsatisfiable: the ∀R.B ∧ ∃R.C ∧ Disjoint(B,C) clash
    assert_unsat(&c, "http://t/A");
    // B and C individually are satisfiable (they just can't co-occur)
    assert_sat(&c, "http://t/B");
    assert_sat(&c, "http://t/C");
}

// ══════════════════════════════════════════════════════════════════════════════
// 3. Reasoning-by-cases → unsat
//
//   A ⊑ B ⊔ C,  B ⊑ ⊥,  C ⊑ ⊥  ⟹  A ⊑ ⊥
//
//   A can only be B or C, but both are already unsatisfiable.
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn reasoning_by_cases_unsat() {
    let c = classify_alch_with_internal(
        r"    Declaration(Class(:A))
    Declaration(Class(:B))
    Declaration(Class(:C))
    SubClassOf(:A ObjectUnionOf(:B :C))
    SubClassOf(:B owl:Nothing)
    SubClassOf(:C owl:Nothing)",
    );
    assert_unsat(&c, "http://t/A");
    assert_unsat(&c, "http://t/B");
    assert_unsat(&c, "http://t/C");
}

// ══════════════════════════════════════════════════════════════════════════════
// 4. Role-hierarchy ∀-propagation
//
//   A ⊑ ∀S.B,  A ⊑ ∃R.C,  SubObjectPropertyOf(R, S)  ⟹  C ⊑ B
//   (every R-successor is also an S-successor, so ∀S.B forces B on it)
//
//   The propagation path: ∀S.B on A + R⊑S means ∀R.B holds too; the ∃R.C
//   successor's context derives B, so C ⊑ B in the hierarchy.
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn role_hierarchy_forall_propagation() {
    let c = classify_alch_with_internal(
        r"    Declaration(Class(:A))
    Declaration(Class(:B))
    Declaration(Class(:C))
    Declaration(ObjectProperty(:R))
    Declaration(ObjectProperty(:S))
    SubClassOf(:A ObjectAllValuesFrom(:S :B))
    SubClassOf(:A ObjectSomeValuesFrom(:R :C))
    SubObjectPropertyOf(:R :S)",
    );
    // The R-successor of A is constrained by ∀S.B (via R⊑S), so C ⊑ B.
    assert_subsumes(&c, "http://t/C", "http://t/B");
}

// ══════════════════════════════════════════════════════════════════════════════
// 5. ⊥-propagation up ∃
//
//   A ⊑ ∃R.C,  C ⊑ ⊥  ⟹  A ⊑ ⊥
//
//   A requires an R-filler of type C, but C is unsatisfiable — no model exists.
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn bot_propagation_up_exists() {
    let c = classify_alch_with_internal(
        r"    Declaration(Class(:A))
    Declaration(Class(:C))
    Declaration(ObjectProperty(:R))
    SubClassOf(:A ObjectSomeValuesFrom(:R :C))
    SubClassOf(:C owl:Nothing)",
    );
    assert_unsat(&c, "http://t/A");
    assert_unsat(&c, "http://t/C");
}

// ══════════════════════════════════════════════════════════════════════════════
// 6. Pure-EL still correct
//
//   A ⊑ ∃R.B,  B ⊑ C,  ∃R.C ⊑ D  ⟹  A ⊑ D
//
//   This is a pure-EL entailment (no disjunction or ∀) — the CB engine must
//   still handle it correctly (the EL saturator's domain is a subset).
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn pure_el_subsumption_chain() {
    let c = classify_alch_with_internal(
        r"    Declaration(Class(:A))
    Declaration(Class(:B))
    Declaration(Class(:C))
    Declaration(Class(:D))
    Declaration(ObjectProperty(:R))
    SubClassOf(:A ObjectSomeValuesFrom(:R :B))
    SubClassOf(:B :C)
    SubClassOf(ObjectSomeValuesFrom(:R :C) :D)",
    );
    assert_subsumes(&c, "http://t/A", "http://t/D");
}

// ══════════════════════════════════════════════════════════════════════════════
// 7. FP guard — a non-entailment must stay absent
//
//   A ⊑ B ⊔ C — without knowing anything about D, A ⊑ D must NOT hold.
//   (Contrasts with test 1 where B⊑D ∧ C⊑D forces it.)
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn no_spurious_subsumption_fp_guard() {
    let c = classify_alch_with_internal(
        r"    Declaration(Class(:A))
    Declaration(Class(:B))
    Declaration(Class(:C))
    Declaration(Class(:D))
    SubClassOf(:A ObjectUnionOf(:B :C))",
    );
    // D is unrelated; A ⊑ D must NOT hold
    assert_not_subsumes(&c, "http://t/A", "http://t/D");
    // And B ⊑ D, C ⊑ D must NOT hold either
    assert_not_subsumes(&c, "http://t/B", "http://t/D");
    assert_not_subsumes(&c, "http://t/C", "http://t/D");
}

// ══════════════════════════════════════════════════════════════════════════════
// 8. ∀-transitivity with role hierarchy and ⊥ clash
//
//   A ⊑ ∃R.C,  C ⊑ ∃S.E,  SubObjectPropertyOf(R,T), SubObjectPropertyOf(S,T),
//   A ⊑ ∀T.B,  DisjointClasses(E, B)  ⟹  A ⊑ ⊥
//
//   Tests multi-hop ∀-propagation: the ∀T.B constraint on A must propagate
//   down through the R-successor (C) and S-successor (E), showing E ⊑ B
//   (from ∀T.B via T-edges), which contradicts Disjoint(E,B).
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn forall_propagation_multi_hop_clash_unsat() {
    let c = classify_alch_with_internal(
        r"    Declaration(Class(:A))
    Declaration(Class(:B))
    Declaration(Class(:C))
    Declaration(Class(:E))
    Declaration(ObjectProperty(:R))
    Declaration(ObjectProperty(:S))
    Declaration(ObjectProperty(:T))
    SubClassOf(:A ObjectSomeValuesFrom(:R :C))
    SubClassOf(:C ObjectSomeValuesFrom(:S :E))
    SubObjectPropertyOf(:R :T)
    SubObjectPropertyOf(:S :T)
    SubClassOf(:A ObjectAllValuesFrom(:T :B))
    DisjointClasses(:E :B)",
    );
    // A is unsatisfiable: E must also be B (via ∀T.B + S⊑T) but Disjoint(E,B)
    assert_unsat(&c, "http://t/A");
}

// ══════════════════════════════════════════════════════════════════════════════
// 9. Conjunction on the left + disjunction on the right
//
//   A ⊓ B ⊑ C ⊔ D,  A ⊓ B ⊑ E  (where C, D, E unrelated)
//   Tests that the CB engine handles conjunctive premises correctly.
//   ⟹  every A ⊓ B individual is in C ⊔ D (a disjunctive necessary condition)
//   but we cannot conclude A ⊑ C or A ⊑ D alone.
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn conjunction_premise_disjunction_head() {
    let c = classify_alch_with_internal(
        r"    Declaration(Class(:A))
    Declaration(Class(:B))
    Declaration(Class(:C))
    Declaration(Class(:D))
    Declaration(Class(:E))
    SubClassOf(ObjectIntersectionOf(:A :B) ObjectUnionOf(:C :D))
    SubClassOf(ObjectIntersectionOf(:A :B) :E)",
    );
    // A alone or B alone does NOT entail C, D, or E (no FP without the conjunction)
    assert_not_subsumes(&c, "http://t/A", "http://t/C");
    assert_not_subsumes(&c, "http://t/A", "http://t/D");
    assert_not_subsumes(&c, "http://t/A", "http://t/E");
    assert_not_subsumes(&c, "http://t/B", "http://t/C");
    assert_not_subsumes(&c, "http://t/B", "http://t/D");
    assert_not_subsumes(&c, "http://t/B", "http://t/E");
}
