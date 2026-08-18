//! Functional object-property ENFORCEMENT in the tableau + hypertableau wedge
//! (consistency / ABox-merge path).
//!
//! GAP: `FunctionalObjectProperty(R)` → `Axiom::FunctionalRole(R)` is enforced
//! by the EL saturator (classify) but was DROPPED by the wedge clausifier and
//! never translated to `≤1 R` for the main tableau. So consistency /
//! ABox-merge missed functional-merge clashes. The fix emits a derived
//! role-triggered GCI `∃R.⊤ ⊑ ≤1 R` at convert time — FORWARD unconditionally, and
//! INVERSE under `RUSTDL_INVERSE_FUNC_MAX` (default OFF, added 2026-08-18; the
//! inverse section below records why the old "deferred sound MISS" reading was wrong).
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
    consistent_engine_only_with_env(body, &[])
}

/// As [`consistent_engine_only`] but sets additional env vars for the duration.
///
/// **Takes `ENV_MUTEX` itself — do NOT lock outside and call this.** Both this and
/// `consistent_engine_only` acquire the lock, and `std::sync::Mutex` is not reentrant;
/// a sibling test file deadlocked its whole binary on exactly that mistake on
/// 2026-08-18, hanging silently rather than failing.
fn consistent_engine_only_with_env(body: &str, env: &[(&'static str, &str)]) -> bool {
    let _serial = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _abox = SetEnvGuard::set("RUSTDL_ABOX_CHECK", "0");
    let _extra: Vec<SetEnvGuard> = env.iter().map(|(k, v)| SetEnvGuard::set(k, v)).collect();
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

// ─── INVERSE-FUNCTIONAL: predecessor-merge — THE SENTINELS TRIPPED ────
//
// REWRITTEN 2026-08-18. Both sentinels below were `#[ignore]`d on the premise that
// "the engine does not perform the `≤1 R⁻` predecessor merge, so emitting
// `∃R⁻.⊤ ⊑ ≤1 R⁻` would be a silent no-op." **That premise is false, and it was
// load-bearing** — it is the stated reason the inverse GCI went unwritten.
//
// Measured, this file, `RUSTDL_ABOX_CHECK=0` throughout:
//
// | probe | ABOX_SATURATION=1 | =0 | verdict |
// |---|---|---|---|
// | explicit `≤1 r⁻` (sentinel 2) | clash | **clash** | the WEDGE does the merge |
// | `InverseFunctional(r)` (sentinel 1) | clash | consistent | needed the pre-check |
// | sentinel 1 + `RUSTDL_INVERSE_FUNC_MAX=1` | clash | **clash** | GCI = sole trigger |
//
// So the engine has performed inverse-role predecessor merges since
// `RUSTDL_INVERSE_FUNC_MERGE` (default ON, 2026-07-11); what was missing was any
// `≤1 r⁻` constraint to TRIGGER it, because `derive_functional_max_cardinality`
// emitted the derived GCI for `FunctionalRole` only. The bottom row is the
// discriminating experiment: with the ABox pre-check off, the wedge is the only
// route, and the flag alone flips the verdict.
//
// Consequence for reading the two tests: sentinel 1 passes AT THE DEFAULT via the
// ABox-saturation pre-check, NOT via the calculus this file claims to isolate — the
// `RUSTDL_ABOX_CHECK=0` guard disables the A1 pre-check but NOT `abox_saturation`.
// The wedge-only variant is therefore the one that pins the fix.
//
// Both sentinels have been un-`#[ignore]`d. Sentinel 2's assertion is FLIPPED, exactly
// as its own doc comment instructed for this moment. They had been out of date for
// weeks, invisibly, because an `#[ignore]`d test that starts passing says nothing.

/// `InverseFunctional(R)` ⇒ `≤1 R⁻`: the node `c` has at most one R-predecessor.
/// `R(a,c)`, `R(b,c)` force `a = b`; `a:M`, `b:F`, `M`,`F` disjoint ⇒ no model.
///
/// **Un-`#[ignore]`d 2026-08-18 — passes at the DEFAULT.** But read the attribution
/// before trusting it as calculus coverage: it is the `abox_saturation` pre-check that
/// finds this clash, not the tableau/wedge (`RUSTDL_ABOX_SATURATION=0` makes it fail).
/// `inverse_functional_predecessor_merge_needs_the_derived_gci` below is the variant
/// that pins the calculus.
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

/// An EXPLICIT `ObjectMaxCardinality(1, ObjectInverseOf(r))` at `c` — the desugared
/// form of inverse-functionality, with NO `InverseFunctionalObjectProperty` axiom and
/// so independent of any translation — is INCONSISTENT.
///
/// **Assertion FLIPPED 2026-08-18**, following this test's own former instruction
/// ("tripping it means the engine learned inverse-role predecessor merging … this
/// should flip to `assert!(!consistent...)`"). It now pins the positive capability:
/// the wedge DOES perform the `≤1 R⁻` predecessor merge, with the `abox_saturation`
/// pre-check either on or off. That capability is what makes the derived
/// `∃r⁻.⊤ ⊑ ≤1 r⁻.⊤` GCI worth emitting rather than a no-op.
#[test]
fn inverse_max_cardinality_explicit_merges_predecessors() {
    assert!(
        !consistent_engine_only(
            r"    Declaration(ObjectProperty(:r))
    Declaration(Class(:C)) Declaration(Class(:M)) Declaration(Class(:F))
    Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))
    Declaration(NamedIndividual(:c))
    SubClassOf(:C ObjectMaxCardinality(1 ObjectInverseOf(:r)))
    ClassAssertion(:C :c)
    ObjectPropertyAssertion(:r :a :c)
    ObjectPropertyAssertion(:r :b :c)
    ClassAssertion(:M :a)
    ClassAssertion(:F :b)
    DisjointClasses(:M :F)"
        ),
        "explicit ≤1 r⁻ at c must merge a and b, and M/F disjoint ⇒ INCONSISTENT. \
         If this fails the wedge's predecessor-walking merge has regressed, and the \
         derived ∃r⁻.⊤ ⊑ ≤1 r⁻.⊤ GCI (RUSTDL_INVERSE_FUNC_MAX) has become a no-op"
    );
}

/// **The discriminating experiment**, and the canary that actually pins the fix.
///
/// Same ontology as `inverse_functional_predecessor_merge_inconsistent`, but with
/// `RUSTDL_ABOX_SATURATION=0` so the ABox pre-check cannot answer it and the wedge is
/// the only route. Measured: the flag alone flips the verdict —
/// `INVERSE_FUNC_MAX=0` ⇒ consistent (the MISS), `=1` ⇒ inconsistent.
///
/// That isolates the mechanism: the merge was always there, the `≤1 r⁻` trigger was not.
/// See `docs/known-limitations/realize-drops-derived-individual-equality.md`.
#[test]
fn inverse_functional_predecessor_merge_needs_the_derived_gci() {
    const BODY: &str = r"    Declaration(ObjectProperty(:r))
    Declaration(Class(:M)) Declaration(Class(:F))
    Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))
    Declaration(NamedIndividual(:c))
    InverseFunctionalObjectProperty(:r)
    ObjectPropertyAssertion(:r :a :c)
    ObjectPropertyAssertion(:r :b :c)
    ClassAssertion(:M :a)
    ClassAssertion(:F :b)
    DisjointClasses(:M :F)";
    assert!(
        consistent_engine_only_with_env(
            BODY,
            &[
                ("RUSTDL_ABOX_SATURATION", "0"),
                ("RUSTDL_INVERSE_FUNC_MAX", "0")
            ],
        ),
        "CONTROL: with the ABox pre-check off and the flag off, the wedge has no ≤1 r⁻ \
         constraint to merge on, so this is a sound MISS (reports consistent). If this \
         fails, some OTHER route now derives it and the experiment below no longer \
         isolates the GCI."
    );
    assert!(
        !consistent_engine_only_with_env(
            BODY,
            &[
                ("RUSTDL_ABOX_SATURATION", "0"),
                ("RUSTDL_INVERSE_FUNC_MAX", "1")
            ],
        ),
        "RUSTDL_INVERSE_FUNC_MAX=1 emits ∃r⁻.⊤ ⊑ ≤1 r⁻.⊤, which triggers the wedge's \
         predecessor merge ⇒ a=b ⇒ M⊓F ⇒ INCONSISTENT. If this fails, the derived GCI \
         is no longer reaching the wedge."
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
