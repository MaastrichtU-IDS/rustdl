//! Drives the preserved adversarial-review fixtures for the `DKey` collapse/broadcast
//! split. See the README beside the fixtures for what each one guards, and
//! `docs/superpowers/specs/2026-07-30-dkey-collapse-vs-broadcast-design.md` R1–R6.
//!
//! These pin CONSUMABLE clashes: every one must keep its verdict when the split is
//! enabled. A flip is a lost clash, i.e. a completeness regression.

#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use std::fs;
use std::io::Cursor;
use std::path::Path;

const DIR: &str = "tests/fixtures/dkey_collapse_broadcast";

fn load(name: &str) -> SetOntology<RcStr> {
    let path = Path::new(DIR).join(format!("{name}.ofn"));
    let src = fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let mut reader = Cursor::new(src);
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut reader, ParserConfiguration::default()).expect("parse ofn");
    onto
}

/// Helper: assert the expected verdict for one fixture.
///
/// - `expect_inconsistent == true` → the whole `KB` is inconsistent; asserts
///   `is_consistent` returns `Ok(false)`.
/// - `expect_inconsistent == false` → the `KB` is consistent; asserts
///   `is_consistent` returns `Ok(true)` **and** that the number of unsatisfiable
///   named classes equals `expect_unsat_count`.
fn check(stem: &str, expect_inconsistent: bool, expect_unsat_count: usize) {
    let onto = load(stem);
    let consistent = owl_dl_reasoner::is_consistent(&onto)
        .unwrap_or_else(|e| panic!("{stem}: is_consistent error: {e}"));
    if expect_inconsistent {
        assert!(
            !consistent,
            "{stem}: expected KB inconsistent but got consistent"
        );
    } else {
        assert!(
            consistent,
            "{stem}: expected KB consistent but got inconsistent"
        );
        let cls = owl_dl_reasoner::classify(&onto)
            .unwrap_or_else(|e| panic!("{stem}: classify error: {e}"));
        let unsat = cls.unsatisfiable_classes();
        assert_eq!(
            unsat.len(),
            expect_unsat_count,
            "{stem}: expected {expect_unsat_count} unsat class(es) but got {} — {:?}",
            unsat.len(),
            unsat
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Individual tests — one per fixture so a failure in one does not hide others.
// ─────────────────────────────────────────────────────────────────────────────

/// broadcast×broadcast: two disjoint `DataPropertyRange` on one property
/// meet on every successor.
#[test]
fn two_disjoint_ranges() {
    check("two-disjoint-ranges", false, 1);
}

/// broadcast×value — the D11b flagship clash. A per-component drop kills this.
#[test]
fn range_vs_value_d11b_flagship() {
    check("range-vs-value-d11b-flagship", false, 1);
}

/// broadcast×broadcast, class-expression form.
#[test]
fn two_ranges_class_unsat() {
    check("two-ranges-class-unsat", false, 1);
}

/// Occurrence-position rule (R2). `DataOneOf("a")` interns to the SAME `ClassId`
/// as an assertion's key, so "singleton ⇒ value key" regresses this.
#[test]
fn exists_plus_two_forall_dataoneof() {
    check("exists-plus-two-forall-dataoneof", false, 1);
}

/// broadcast×value via the `ABox`.
#[test]
fn range_vs_datahasvalue() {
    check("range-vs-datahasvalue", true, 0);
}

/// broadcast rides DOWN the property hierarchy; also why the union-find must stay
/// gated on the full merge set (R3).
#[test]
fn range_on_super_value_on_sub() {
    check("range-on-super-value-on-sub", true, 0);
}

/// COLLAPSE must be closed downward (R4).
#[test]
fn functional_super_values_on_sub() {
    check("functional-super-values-on-sub", true, 0);
}

/// R4, three levels, middle role broadcast-only.
#[test]
fn functional_3_level() {
    check("functional-3-level", true, 0);
}

/// R4 via two sub-roles sharing a functional super.
#[test]
fn downward_closure_two_subs() {
    check("downward-closure-two-subs", false, 1);
}

/// NEGATIVE CONTROL: the downward closure is NOT needed upward. Must stay
/// satisfiable with 0 unsatisfiable classes. If this ever goes unsat, something
/// over-approximates.
#[test]
fn negative_functional_sub_values_on_super() {
    check("NEGATIVE-functional-sub-values-on-super", false, 0);
}

/// PRE-EXISTING MISS — pinned so it is not mistaken for a regression.
///
/// `∀f.DataOneOf` on a super + conflicting value on a sub is MISSED by rustdl:
/// the `KB` is logically inconsistent (as Konclude/HermiT derive) but rustdl
/// currently reports it consistent. This is an asymmetric `∀`-propagation gap
/// down the data-property hierarchy, not introduced by the collapse/broadcast
/// split. The expectation encodes today's (wrong but stable) behaviour
/// deliberately; do NOT change it to the logically-correct `true` — that would
/// only pass once the gap is closed.
#[test]
fn known_miss_forall_super_value_sub() {
    check("KNOWN-MISS-forall-super-value-sub", false, 0);
}
