//! Sequoia (ordered, S1) vs unordered B1 differential parity on ALCH.
//!
//! The S1 make-or-break gate (§6 of the Sequoia re-architecture design): on
//! every ALCH input where the sound+complete unordered B1 engine returns a
//! hierarchy, the Sequoia ordered engine must return the **same** hierarchy —
//! FP=0 (nothing the unordered engine lacks) AND MISSED=0 (matches B1).
//!
//! Both engines run in-process on the SAME normalized input via the explicit
//! `classify_unordered` / `classify_sequoia` entry points, so the comparison is
//! independent of `RUSTDL_CB_CALCULUS`. The synthetic ALCH ontologies exercise
//! disjunction, ∀, role hierarchy, nested ∃, conjunctive premises, and the
//! by-cases inference Slice-0's ordering broke.
//!
//! Run: `cargo test -p owl-dl-cb --test cb_sequoia_diff`.

#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_cb::{CbHierarchy, CbOutcome};
use owl_dl_core::convert::convert_ontology;
use std::io::Cursor;

const PFX: &str = "Prefix(:=<http://t/>)\nPrefix(owl:=<http://www.w3.org/2002/07/owl#>)\n";

fn parse(body: &str) -> owl_dl_core::ontology::InternalOntology {
    let src = format!("{PFX}Ontology(<http://t/x>\n{body}\n)\n");
    let mut reader = Cursor::new(src);
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut reader, ParserConfiguration::default()).expect("OFN parse error");
    convert_ontology(&onto).expect("convert_ontology error")
}

fn hier(outcome: CbOutcome) -> CbHierarchy {
    match outcome {
        CbOutcome::Classified(h) => h,
        CbOutcome::OutOfFragment(reason) => panic!("unexpected OutOfFragment: {reason}"),
    }
}

/// Serializes every test in this binary that reads OR writes
/// `RUSTDL_CB_SECOND_MAXIMAL` (cargo runs `#[test]` fns concurrently within one
/// process, and `classify_sequoia` snapshots the flag via `var_os` at
/// `OrderBuilder::build`). Both the flag-ON parity test and the default-mode
/// fixture tests take this lock, so no test observes another's env mutation.
/// Mirrors the `ENV_MUTEX` prior art in the reasoner crate's tests.
static ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// RAII guard that sets `RUSTDL_CB_SECOND_MAXIMAL=1` for its lifetime and
/// restores the prior value on drop — robust even if a parity assertion panics.
/// It HOLDS the `ENV_MUTEX` for its whole lifetime, so no concurrent test reads
/// the polluted var. Also used (via [`env_serial`]) to fence default-mode tests.
struct SecondMaximalGuard {
    prev: Option<std::ffi::OsString>,
    _serial: std::sync::MutexGuard<'static, ()>,
}

impl SecondMaximalGuard {
    #[allow(unsafe_code)]
    fn set() -> Self {
        let serial = ENV_MUTEX
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let prev = std::env::var_os("RUSTDL_CB_SECOND_MAXIMAL");
        // SAFETY: set_var is unsafe under edition 2024. Held only for one test,
        // serialized via ENV_MUTEX (every flag-reading test also locks it),
        // restored on Drop.
        unsafe { std::env::set_var("RUSTDL_CB_SECOND_MAXIMAL", "1") };
        Self {
            prev,
            _serial: serial,
        }
    }
}

impl Drop for SecondMaximalGuard {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        // SAFETY: see `set`.
        unsafe {
            match &self.prev {
                Some(v) => std::env::set_var("RUSTDL_CB_SECOND_MAXIMAL", v),
                None => std::env::remove_var("RUSTDL_CB_SECOND_MAXIMAL"),
            }
        }
    }
}

/// Take the `ENV_MUTEX` (no mutation) so a DEFAULT-mode `classify_sequoia` test
/// cannot run concurrently with a flag-ON test and read a polluted flag.
fn env_serial() -> std::sync::MutexGuard<'static, ()> {
    ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The ALCH parity fixtures shared by the default and second-maximal differentials.
/// Each `(name, body)` must give `tamed-S1 ≡ B1` in BOTH modes.
fn parity_fixtures() -> Vec<(&'static str, &'static str)> {
    vec![
        (
            "by-cases",
            r"    Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C)) Declaration(Class(:D))
    SubClassOf(:A ObjectUnionOf(:B :C))
    SubClassOf(:B :D)
    SubClassOf(:C :D)",
        ),
        (
            "forall-exists-clash",
            r"    Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C))
    Declaration(ObjectProperty(:R))
    SubClassOf(:A ObjectAllValuesFrom(:R :B))
    SubClassOf(:A ObjectSomeValuesFrom(:R :C))
    DisjointClasses(:B :C)",
        ),
        (
            "role-hier-forall",
            r"    Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C))
    Declaration(ObjectProperty(:R)) Declaration(ObjectProperty(:S))
    SubClassOf(:A ObjectAllValuesFrom(:S :B))
    SubClassOf(:A ObjectSomeValuesFrom(:R :C))
    SubObjectPropertyOf(:R :S)
    DisjointClasses(:B :C)",
        ),
        (
            "nested-exists",
            r"    Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C)) Declaration(Class(:D))
    Declaration(ObjectProperty(:R))
    SubClassOf(:A ObjectSomeValuesFrom(:R ObjectIntersectionOf(:B :C)))
    SubClassOf(:B :D)
    SubClassOf(ObjectSomeValuesFrom(:R ObjectIntersectionOf(:D :C)) owl:Nothing)",
        ),
        (
            "conj-prem-disj-head",
            r"    Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C)) Declaration(Class(:D)) Declaration(Class(:E))
    SubClassOf(ObjectIntersectionOf(:A :B) ObjectUnionOf(:C :D))
    SubClassOf(ObjectIntersectionOf(:A :B) :E)",
        ),
        (
            "15-class-alch",
            r"    Declaration(Class(:C0)) Declaration(Class(:C1)) Declaration(Class(:C2))
    Declaration(Class(:C3)) Declaration(Class(:C4)) Declaration(Class(:C5))
    Declaration(Class(:C6)) Declaration(Class(:C7)) Declaration(Class(:C8))
    Declaration(Class(:C9)) Declaration(Class(:C10)) Declaration(Class(:C11))
    Declaration(Class(:C12)) Declaration(Class(:C13)) Declaration(Class(:C14))
    Declaration(ObjectProperty(:r)) Declaration(ObjectProperty(:s))
    SubObjectPropertyOf(:r :s)
    SubClassOf(:C0 ObjectUnionOf(:C1 :C2))
    SubClassOf(:C1 :C3)
    SubClassOf(:C2 :C3)
    SubClassOf(:C3 :C4)
    SubClassOf(:C4 ObjectUnionOf(:C5 :C6))
    SubClassOf(:C5 :C7)
    SubClassOf(:C6 :C7)
    SubClassOf(:C7 :C8)
    SubClassOf(:C8 ObjectSomeValuesFrom(:r :C9))
    SubClassOf(:C8 ObjectAllValuesFrom(:s :C10))
    SubClassOf(ObjectIntersectionOf(:C9 :C10) :C11)
    SubClassOf(:C11 :C12)
    SubClassOf(ObjectIntersectionOf(:C12 :C9) owl:Nothing)
    SubClassOf(:C13 ObjectUnionOf(:C14 :C8))
    SubClassOf(:C14 :C8)",
        ),
        (
            "no-spurious",
            r"    Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C)) Declaration(Class(:D))
    SubClassOf(:A ObjectUnionOf(:B :C))",
        ),
        (
            "equiv-cycle",
            r"    Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C)) Declaration(Class(:D))
    EquivalentClasses(:A :B)
    SubClassOf(:A ObjectUnionOf(:C :D))
    SubClassOf(:C :B)
    SubClassOf(:D :B)",
        ),
        // ∀-disjunctive by-cases through a successor context: A ⊑ ∃R.⊤,
        // ⊤ ⊑ ∀R.(D1⊔D2), D1⊑E, D2⊑E. Both engines must derive that the R-successor
        // of A is ⊑ E by cases (D1 or D2, both ⊑ E). This exercises exactly the
        // ∀-back-propagation + disjunctive Hyper the second-maximal relaxation
        // touches, and it entails a NON-VACUOUS successor-context subsumption.
        (
            "forall-disjunctive-bycases",
            r"    Declaration(Class(:A)) Declaration(Class(:D1)) Declaration(Class(:D2)) Declaration(Class(:E))
    Declaration(ObjectProperty(:R))
    SubClassOf(:A ObjectSomeValuesFrom(:R owl:Thing))
    SubClassOf(owl:Thing ObjectAllValuesFrom(:R ObjectUnionOf(:D1 :D2)))
    SubClassOf(:D1 :E)
    SubClassOf(:D2 :E)",
        ),
    ]
}

/// Run both engines on `body` and assert identical subsumption + unsat sets,
/// SERIALIZING against flag-mutating tests (default flag state = unset). The
/// flag-ON parity test holds the `ENV_MUTEX` itself and calls
/// [`assert_parity_locked`] directly to avoid re-entrant locking.
fn assert_parity(name: &str, body: &str) {
    let _serial = env_serial();
    assert_parity_locked(name, body);
}

/// Body of [`assert_parity`], assuming the caller already holds `ENV_MUTEX`.
fn assert_parity_locked(name: &str, body: &str) {
    let internal = parse(body);
    let unordered = hier(owl_dl_cb::classify_unordered(&internal));
    let sequoia = hier(owl_dl_cb::classify_sequoia(&internal));

    let fp: Vec<_> = sequoia
        .subsumptions
        .difference(&unordered.subsumptions)
        .collect();
    let missed: Vec<_> = unordered
        .subsumptions
        .difference(&sequoia.subsumptions)
        .collect();
    let unsat_fp: Vec<_> = sequoia.unsat.difference(&unordered.unsat).collect();
    let unsat_missed: Vec<_> = unordered.unsat.difference(&sequoia.unsat).collect();

    assert!(
        fp.is_empty(),
        "[{name}] FALSE POSITIVES (sequoia ⊉ unordered): {fp:?}"
    );
    assert!(
        missed.is_empty(),
        "[{name}] MISSED (unordered has, sequoia lacks): {missed:?}"
    );
    assert!(unsat_fp.is_empty(), "[{name}] unsat FP: {unsat_fp:?}");
    assert!(
        unsat_missed.is_empty(),
        "[{name}] unsat MISSED: {unsat_missed:?}"
    );
}

#[test]
fn diff_disjunctive_by_cases() {
    assert_parity(
        "by-cases",
        r"    Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C)) Declaration(Class(:D))
    SubClassOf(:A ObjectUnionOf(:B :C))
    SubClassOf(:B :D)
    SubClassOf(:C :D)",
    );
}

#[test]
fn diff_forall_exists_clash() {
    assert_parity(
        "forall-exists-clash",
        r"    Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C))
    Declaration(ObjectProperty(:R))
    SubClassOf(:A ObjectAllValuesFrom(:R :B))
    SubClassOf(:A ObjectSomeValuesFrom(:R :C))
    DisjointClasses(:B :C)",
    );
}

#[test]
fn diff_role_hierarchy_forall() {
    assert_parity(
        "role-hier-forall",
        r"    Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C))
    Declaration(ObjectProperty(:R)) Declaration(ObjectProperty(:S))
    SubClassOf(:A ObjectAllValuesFrom(:S :B))
    SubClassOf(:A ObjectSomeValuesFrom(:R :C))
    SubObjectPropertyOf(:R :S)
    DisjointClasses(:B :C)",
    );
}

#[test]
fn diff_nested_existential() {
    assert_parity(
        "nested-exists",
        r"    Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C)) Declaration(Class(:D))
    Declaration(ObjectProperty(:R))
    SubClassOf(:A ObjectSomeValuesFrom(:R ObjectIntersectionOf(:B :C)))
    SubClassOf(:B :D)
    SubClassOf(ObjectSomeValuesFrom(:R ObjectIntersectionOf(:D :C)) owl:Nothing)",
    );
}

#[test]
fn diff_conjunction_premise_disjunction_head() {
    assert_parity(
        "conj-prem-disj-head",
        r"    Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C)) Declaration(Class(:D)) Declaration(Class(:E))
    SubClassOf(ObjectIntersectionOf(:A :B) ObjectUnionOf(:C :D))
    SubClassOf(ObjectIntersectionOf(:A :B) :E)",
    );
}

/// A larger 15-class ALCH gate: a told-subsumption lattice + several
/// disjunctive by-cases + ∀/∃ + role hierarchy + disjointness, all woven
/// together so the order construction is exercised at depth.
#[test]
fn diff_synthetic_15_class_alch() {
    assert_parity(
        "15-class-alch",
        r"    Declaration(Class(:C0)) Declaration(Class(:C1)) Declaration(Class(:C2))
    Declaration(Class(:C3)) Declaration(Class(:C4)) Declaration(Class(:C5))
    Declaration(Class(:C6)) Declaration(Class(:C7)) Declaration(Class(:C8))
    Declaration(Class(:C9)) Declaration(Class(:C10)) Declaration(Class(:C11))
    Declaration(Class(:C12)) Declaration(Class(:C13)) Declaration(Class(:C14))
    Declaration(ObjectProperty(:r)) Declaration(ObjectProperty(:s))
    SubObjectPropertyOf(:r :s)
    SubClassOf(:C0 ObjectUnionOf(:C1 :C2))
    SubClassOf(:C1 :C3)
    SubClassOf(:C2 :C3)
    SubClassOf(:C3 :C4)
    SubClassOf(:C4 ObjectUnionOf(:C5 :C6))
    SubClassOf(:C5 :C7)
    SubClassOf(:C6 :C7)
    SubClassOf(:C7 :C8)
    SubClassOf(:C8 ObjectSomeValuesFrom(:r :C9))
    SubClassOf(:C8 ObjectAllValuesFrom(:s :C10))
    SubClassOf(ObjectIntersectionOf(:C9 :C10) :C11)
    SubClassOf(:C11 :C12)
    SubClassOf(ObjectIntersectionOf(:C12 :C9) owl:Nothing)
    SubClassOf(:C13 ObjectUnionOf(:C14 :C8))
    SubClassOf(:C14 :C8)",
    );
}

/// FP guard differential: pure disjunction with no convergent subsumer — both
/// engines must report NO new subsumptions.
#[test]
fn diff_no_spurious() {
    assert_parity(
        "no-spurious",
        r"    Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C)) Declaration(Class(:D))
    SubClassOf(:A ObjectUnionOf(:B :C))",
    );
}

/// Equivalent classes (told-subsumption cycle) — exercises the depth fixpoint's
/// cycle handling in the order construction.
#[test]
fn diff_equivalent_cycle() {
    assert_parity(
        "equiv-cycle",
        r"    Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C)) Declaration(Class(:D))
    EquivalentClasses(:A :B)
    SubClassOf(:A ObjectUnionOf(:C :D))
    SubClassOf(:C :B)
    SubClassOf(:D :B)",
    );
}

/// Default (single-maximal) mode: the new ∀-disjunctive by-cases fixture must
/// also give B1 ≡ S1 (regression guard so the fixture isn't only checked ON).
#[test]
fn diff_forall_disjunctive_bycases() {
    let (name, body) = parity_fixtures()
        .into_iter()
        .find(|(n, _)| *n == "forall-disjunctive-bycases")
        .unwrap();
    assert_parity(name, body);
}

/// CANDIDATE-1 GATE 1 (completeness, the crown jewel): with
/// `RUSTDL_CB_SECOND_MAXIMAL=1`, tamed-S1 ≡ B1 on EVERY parity fixture —
/// no FP (a subsumption/unsat B1 lacks) and no MISSED (one B1 has, S1 lacks).
/// If this fails, the relaxation is WRONG — do NOT weaken the assertion.
#[test]
fn second_maximal_preserves_b1_parity() {
    // Holds ENV_MUTEX for the whole test; call the lock-free body to avoid
    // re-entrant locking.
    let _guard = SecondMaximalGuard::set();
    for (name, body) in parity_fixtures() {
        assert_parity_locked(name, body);
    }
}
