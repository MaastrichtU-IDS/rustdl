//! Functional / inverse-functional object-property ENFORCEMENT in the
//! tableau + hypertableau wedge (consistency / ABox-merge path).
//!
//! GAP: `FunctionalObjectProperty(R)` → `Axiom::FunctionalRole(R)` is enforced
//! by the EL saturator (classify) but was DROPPED by the wedge clausifier and
//! never translated to `≤1 R` for the main tableau. So consistency /
//! ABox-merge missed functional-merge clashes. The fix emits a derived
//! role-triggered GCI `∃R.⊤ ⊑ ≤1 R` (and `∃R⁻.⊤ ⊑ ≤1 R⁻` for
//! inverse-functional) at convert time.
//!
//! These tests run engine-level via `is_consistent` with
//! `RUSTDL_ABOX_CHECK=0` so the A1 P8 ABox PRE-CHECK is disabled — isolating
//! the tableau/wedge calculus (the A1 pre-check would otherwise mask the gap
//! for the shallow forward case).
//!
//! NEGATIVES-FIRST: a spurious `Inconsistent` marks every class unsatisfiable —
//! the catastrophic FP. The `*_consistent` controls guard that the new `≤1`
//! merges fire ONLY on a genuine disjoint-witness clash.
//!
//! Run: `cargo test -p owl-dl-reasoner --test functional_enforcement`.

#![allow(clippy::unwrap_used, clippy::doc_markdown)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::is_consistent;
use std::io::Cursor;

const PFX: &str = r"Prefix(:=<http://t/>)
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)
";

// Env-mutation plumbing: serialize RUSTDL_ABOX_CHECK=0 against other
// env-mutating tests, restore on Drop.
static ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct SetEnvGuard {
    key: &'static str,
    prior: Option<std::ffi::OsString>,
}

impl SetEnvGuard {
    #[allow(unsafe_code)]
    fn set(key: &'static str, value: &str) -> Self {
        let prior = std::env::var_os(key);
        // SAFETY: set_var is unsafe under edition 2024. Held only for one
        // test, serialized via ENV_MUTEX, restored on Drop.
        unsafe {
            std::env::set_var(key, value);
        }
        Self { key, prior }
    }
}

impl Drop for SetEnvGuard {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        // SAFETY: see SetEnvGuard::set.
        unsafe {
            match &self.prior {
                Some(v) => std::env::set_var(self.key, v),
                None => std::env::remove_var(self.key),
            }
        }
    }
}

/// Parse + consistency-check `body` with the A1 ABox pre-check DISABLED, so the
/// verdict comes from the tableau/wedge calculus alone.
fn consistent_engine_only(body: &str) -> bool {
    let _serial = ENV_MUTEX.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let _abox = SetEnvGuard::set("RUSTDL_ABOX_CHECK", "0");
    let src = format!("{PFX}Ontology(<http://t/x>\n{body}\n)\n");
    let mut reader = Cursor::new(src);
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut reader, ParserConfiguration::default()).expect("parse ofn");
    is_consistent(&onto).expect("is_consistent")
}

// ─── FORWARD functional: must be INCONSISTENT (the gap) ──────────────

/// `Functional(R)` forces a single R-successor; `A` requires an R-successor
/// that is both `M` and `F`; `M`,`F` disjoint ⇒ no model. WAS reported
/// consistent (functional dropped).
#[test]
fn forward_functional_merge_disjoint_inconsistent() {
    assert!(
        !consistent_engine_only(
            r"    Declaration(ObjectProperty(:r))
    Declaration(Class(:A)) Declaration(Class(:M)) Declaration(Class(:F))
    Declaration(NamedIndividual(:a))
    FunctionalObjectProperty(:r)
    SubClassOf(:A ObjectIntersectionOf(ObjectSomeValuesFrom(:r :M) ObjectSomeValuesFrom(:r :F)))
    DisjointClasses(:M :F)
    ClassAssertion(:A :a)"
        ),
        "Functional(r) + A⊑∃r.M⊓∃r.F + Disjoint(M,F) + A(a) must be INCONSISTENT \
         (the ≤1 r merge forces the disjoint witnesses to coincide)"
    );
}

// ─── INVERSE-FUNCTIONAL: predecessor-merge, must be INCONSISTENT ──────

/// `InverseFunctional(R)` ⇒ `≤1 R⁻`: the node `c` has at most one R-predecessor.
/// `R(a,c)`, `R(b,c)` force `a = b`; `a:M`, `b:F`, `M`,`F` disjoint ⇒ no model.
/// This is the predecessor-merge path (untested in the wedge edge rep).
#[test]
fn inverse_functional_predecessor_merge_inconsistent() {
    assert!(
        !consistent_engine_only(
            r"    Declaration(ObjectProperty(:r))
    Declaration(Class(:M)) Declaration(Class(:F))
    Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))
    Declaration(NamedIndividual(:c))
    InverseFunctionalObjectProperty(:r)
    ObjectPropertyAssertion(:r :a :c)
    ObjectPropertyAssertion(:r :b :c)
    ClassAssertion(:M :a)
    ClassAssertion(:F :b)
    DisjointClasses(:M :F)"
        ),
        "InverseFunctional(r) + r(a,c) + r(b,c) + a:M + b:F + Disjoint(M,F) must be \
         INCONSISTENT (≤1 r⁻ at c merges a and b)"
    );
}

// ─── CONTROLS: must stay CONSISTENT (FP guard) ───────────────────────

/// Functional R, two R-successors with the SAME (non-disjoint) type ⇒ the
/// `≤1 R` merge is harmless (M⊓M = M, no clash). Must stay CONSISTENT.
#[test]
fn forward_functional_nondisjoint_consistent() {
    assert!(
        consistent_engine_only(
            r"    Declaration(ObjectProperty(:r))
    Declaration(Class(:A)) Declaration(Class(:M))
    Declaration(NamedIndividual(:a))
    FunctionalObjectProperty(:r)
    SubClassOf(:A ObjectIntersectionOf(ObjectSomeValuesFrom(:r :M) ObjectSomeValuesFrom(:r :M)))
    ClassAssertion(:A :a)",
        ),
        "Functional(r) + two ∃r.M (same type) must stay CONSISTENT (no spurious clash)"
    );
}

/// Inverse-functional, two R-predecessors with NON-disjoint types ⇒ merge is
/// harmless. Must stay CONSISTENT.
#[test]
fn inverse_functional_nondisjoint_consistent() {
    assert!(
        consistent_engine_only(
            r"    Declaration(ObjectProperty(:r))
    Declaration(Class(:M))
    Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))
    Declaration(NamedIndividual(:c))
    InverseFunctionalObjectProperty(:r)
    ObjectPropertyAssertion(:r :a :c)
    ObjectPropertyAssertion(:r :b :c)
    ClassAssertion(:M :a)
    ClassAssertion(:M :b)",
        ),
        "InverseFunctional(r) + two predecessors both :M must stay CONSISTENT"
    );
}

/// Control: WITHOUT functionality, the same disjoint-witness shape is
/// satisfiable (two distinct successors, no merge). Guards that the
/// inconsistency above is genuinely caused by the functional enforcement.
#[test]
fn forward_nonfunctional_disjoint_consistent() {
    assert!(
        consistent_engine_only(
            r"    Declaration(ObjectProperty(:r))
    Declaration(Class(:A)) Declaration(Class(:M)) Declaration(Class(:F))
    Declaration(NamedIndividual(:a))
    SubClassOf(:A ObjectIntersectionOf(ObjectSomeValuesFrom(:r :M) ObjectSomeValuesFrom(:r :F)))
    DisjointClasses(:M :F)
    ClassAssertion(:A :a)",
        ),
        "WITHOUT Functional(r): two distinct r-successors, no merge ⇒ CONSISTENT"
    );
}
