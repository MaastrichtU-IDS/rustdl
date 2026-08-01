//! Domain absorption (`RUSTDL_DOMAIN_ABSORPTION`, default OFF) — verdict-level
//! canaries.
//!
//! `absorb::as_trigger` recognises only `Not(Atomic)` / `Not(Nominal)`, so
//! `ObjectPropertyDomain(R, D)` — lowered to `∃R.⊤ ⊑ D`, internalised to
//! `⊤ ⊑ ∀R.⊥ ⊔ D` — lands in `residual_gcis`, a global disjunction applied at
//! every node. Domain absorption rewrites it as an unguarded `RoleRule` on
//! `R⁻` so it fires on edge creation instead.
//!
//! `(≥1 R) ⊑ C` is *logically identical* to `ObjectPropertyDomain(R, C)`, so
//! the change must be **verdict-preserving**: every test here asserts the same
//! answer with the flag ON and OFF.
//!
//! # NEGATIVES FIRST
//!
//! The two shapes that must NOT be absorbed carry the entire soundness risk,
//! and each gets a **false-positive** canary — an ontology that stays
//! CONSISTENT and would flip INCONSISTENT if the recogniser were widened:
//!
//! * [`min_two_antecedent_with_one_successor_stays_consistent`] — `Max(k≥1)`.
//!   Absorbing `(≥2 R) ⊑ C` as a domain rule fires it at the *first*
//!   successor, deriving `C` where only one edge exists. **UNSOUND.**
//! * [`qualified_antecedent_with_other_filler_stays_consistent`] — `All(R, D)`
//!   with `D ≠ ⊥`. Absorbing `∃R.E ⊑ C` as a domain rule drops the filler
//!   check, deriving `C` from an edge whose successor is not an `E`.
//!
//! Every test runs on the **main tableau** (`RUSTDL_HYPERTABLEAU=0`,
//! `RUSTDL_WEDGE_CONSISTENCY=0`) and with both ABox pre-checks off: the
//! clausifier works from `InternalOntology` directly, never from the absorbed
//! TBox, so a wedge-answered query would be blind to this feature.
//!
//! Run: `cargo test -p owl-dl-reasoner --test domain_absorption`.

#![allow(clippy::unwrap_used, clippy::doc_markdown)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::is_consistent;
use std::io::Cursor;

const PFX: &str = r"Prefix(:=<http://t/>)
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
";

static ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct SetEnvGuard {
    key: &'static str,
    prior: Option<std::ffi::OsString>,
}

impl SetEnvGuard {
    #[allow(unsafe_code)]
    fn set(key: &'static str, value: &str) -> Self {
        let prior = std::env::var_os(key);
        // SAFETY: `set_var` is unsafe under edition 2024. Held for one test,
        // serialized via ENV_MUTEX, restored on Drop.
        unsafe { std::env::set_var(key, value) };
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

/// Consistency via the **main tableau** with `RUSTDL_DOMAIN_ABSORPTION` set to
/// `flag`. The wedge and both ABox pre-checks are disabled so the verdict comes
/// from the absorbed-TBox path this feature edits.
fn consistent_tableau(body: &str, flag: &str) -> bool {
    let _serial = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _domain = SetEnvGuard::set("RUSTDL_DOMAIN_ABSORPTION", flag);
    let _hyper = SetEnvGuard::set("RUSTDL_HYPERTABLEAU", "0");
    let _wedge = SetEnvGuard::set("RUSTDL_WEDGE_CONSISTENCY", "0");
    let _abox = SetEnvGuard::set("RUSTDL_ABOX_CHECK", "0");
    let _asat = SetEnvGuard::set("RUSTDL_ABOX_SATURATION", "0");
    let src = format!("{PFX}Ontology(<http://t/x>\n{body}\n)\n");
    let mut reader = Cursor::new(src);
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut reader, ParserConfiguration::default()).expect("parse ofn");
    is_consistent(&onto).expect("is_consistent")
}

/// Assert the same verdict with the flag ON and OFF, and return it.
/// Verdict identity is the core correctness claim of this feature.
#[track_caller]
fn verdict_identical(body: &str) -> bool {
    let off = consistent_tableau(body, "0");
    let on = consistent_tableau(body, "1");
    assert_eq!(
        off, on,
        "domain absorption changed a verdict (OFF={off}, ON={on}) — \
         it is logically identical to ObjectPropertyDomain and must not"
    );
    on
}

// ─── NEGATIVE (false-positive) canaries — must stay CONSISTENT ────────

/// **`Max(k ≥ 1)` must not be absorbed.** `(≥2 R) ⊑ C` with only ONE R-edge
/// does not entail `C(a)`, so `a : ¬C` is consistent. A domain rule fires at
/// the first successor and would derive `C(a)` ⇒ clash ⇒ **false positive**.
#[test]
fn min_two_antecedent_with_one_successor_stays_consistent() {
    assert!(
        verdict_identical(
            r"    Declaration(ObjectProperty(:r)) Declaration(Class(:C)) Declaration(Class(:D))
    Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))
    SubClassOf(ObjectMinCardinality(2 :r) :C)
    DisjointClasses(:C :D)
    ObjectPropertyAssertion(:r :a :b)
    ClassAssertion(:D :a)"
        ),
        "≥2 R antecedent fired at ONE successor = unsound false positive"
    );
}

/// **`All(R, D)` with `D ≠ ⊥` must not be absorbed.** `∃R.E ⊑ C` with an
/// R-successor that is *not* an `E` does not entail `C(a)`. Dropping the
/// filler check would derive `C(a)` ⇒ clash ⇒ **false positive**.
#[test]
fn qualified_antecedent_with_other_filler_stays_consistent() {
    assert!(
        verdict_identical(
            r"    Declaration(ObjectProperty(:r))
    Declaration(Class(:C)) Declaration(Class(:D)) Declaration(Class(:E)) Declaration(Class(:F))
    Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))
    SubClassOf(ObjectSomeValuesFrom(:r :E) :C)
    DisjointClasses(:C :D)
    DisjointClasses(:E :F)
    ObjectPropertyAssertion(:r :a :b)
    ClassAssertion(:F :b)
    ClassAssertion(:D :a)"
        ),
        "qualified ∃R.E antecedent fired without a filler check = false positive"
    );
}

/// **`Max(0, R, C)` with `C ≠ ⊤` must not be absorbed** — the same qualified
/// case written as a `≤0` cardinality, reached from a *qualified*
/// `ObjectMinCardinality(1 :r :E)` antecedent. `¬(≥1 r.E)` is `≤0 r.E`.
/// The one r-successor is an `F`, not an `E`, so `C(a)` is not entailed.
#[test]
fn qualified_max_zero_antecedent_stays_consistent() {
    assert!(
        verdict_identical(
            r"    Declaration(ObjectProperty(:r))
    Declaration(Class(:C)) Declaration(Class(:D)) Declaration(Class(:E)) Declaration(Class(:F))
    Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))
    SubClassOf(ObjectMinCardinality(1 :r :E) :C)
    DisjointClasses(:C :D)
    DisjointClasses(:E :F)
    ObjectPropertyAssertion(:r :a :b)
    ClassAssertion(:F :b)
    ClassAssertion(:D :a)"
        ),
        "≤0 R.C with C ≠ ⊤ fired without a filler check = false positive"
    );
}

/// A domain axiom on an **unrelated** role must not fire. Control against an
/// absorbed rule that ignores its role.
#[test]
fn unrelated_role_domain_stays_consistent() {
    assert!(verdict_identical(
        r"    Declaration(ObjectProperty(:p)) Declaration(ObjectProperty(:r))
    Declaration(Class(:C)) Declaration(Class(:D))
    Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))
    ObjectPropertyDomain(:r :C)
    DisjointClasses(:C :D)
    ObjectPropertyAssertion(:p :a :b)
    ClassAssertion(:D :a)"
    ));
}

/// A domain axiom with **no** edge at all must not fire.
#[test]
fn domain_without_any_edge_stays_consistent() {
    assert!(verdict_identical(
        r"    Declaration(ObjectProperty(:r)) Declaration(Class(:C)) Declaration(Class(:D))
    Declaration(NamedIndividual(:a))
    ObjectPropertyDomain(:r :C)
    DisjointClasses(:C :D)
    ClassAssertion(:D :a)"
    ));
}

/// Direction check: a domain rule constrains the edge **source**, never the
/// target. `r(a,b)` + `b : ¬C` must stay CONSISTENT — if the rewrite forgot to
/// flip the role it would label `b` and clash.
#[test]
fn domain_does_not_constrain_the_edge_target() {
    assert!(verdict_identical(
        r"    Declaration(ObjectProperty(:r)) Declaration(Class(:C)) Declaration(Class(:D))
    Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))
    ObjectPropertyDomain(:r :C)
    DisjointClasses(:C :D)
    ObjectPropertyAssertion(:r :a :b)
    ClassAssertion(:D :b)"
    ));
}

// ─── POSITIVE canaries — must be INCONSISTENT (completeness) ──────────

/// The absorbed rule must still fire: `Domain(r, C)` + `r(a,b)` + `a : ¬C`.
#[test]
fn domain_fires_on_asserted_edge() {
    assert!(!verdict_identical(
        r"    Declaration(ObjectProperty(:r)) Declaration(Class(:C)) Declaration(Class(:D))
    Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))
    ObjectPropertyDomain(:r :C)
    DisjointClasses(:C :D)
    ObjectPropertyAssertion(:r :a :b)
    ClassAssertion(:D :a)"
    ));
}

/// The `ore_ont_3281` source shape — `(≥1 r) ⊑ C` written as a
/// `SubClassOf` over `ObjectMinCardinality(1 …)`, not as an
/// `ObjectPropertyDomain`. Same axiom, must behave the same.
#[test]
fn min_one_antecedent_fires_on_asserted_edge() {
    assert!(!verdict_identical(
        r"    Declaration(ObjectProperty(:r)) Declaration(Class(:C)) Declaration(Class(:D))
    Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))
    SubClassOf(ObjectMinCardinality(1 :r) :C)
    DisjointClasses(:C :D)
    ObjectPropertyAssertion(:r :a :b)
    ClassAssertion(:D :a)"
    ));
}

/// The rule must fire on a **generated** successor too, not just an asserted
/// ABox edge: `a : ∃r.⊤` creates the edge during completion.
#[test]
fn domain_fires_on_generated_successor() {
    assert!(!verdict_identical(
        r"    Declaration(ObjectProperty(:r)) Declaration(Class(:C)) Declaration(Class(:D))
    Declaration(NamedIndividual(:a))
    ObjectPropertyDomain(:r :C)
    DisjointClasses(:C :D)
    ClassAssertion(ObjectSomeValuesFrom(:r owl:Thing) :a)
    ClassAssertion(:D :a)"
    ));
}

/// **Sub-role propagation** must survive the rewrite: `s ⊑ r` and an `s`-edge
/// makes the source a domain node of `r`. The tableau's `edge_satisfies`
/// supplies this; a rewrite that matched role ids exactly would miss it.
#[test]
fn domain_fires_through_a_sub_role() {
    assert!(!verdict_identical(
        r"    Declaration(ObjectProperty(:r)) Declaration(ObjectProperty(:s))
    Declaration(Class(:C)) Declaration(Class(:D))
    Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))
    SubObjectPropertyOf(:s :r)
    ObjectPropertyDomain(:r :C)
    DisjointClasses(:C :D)
    ObjectPropertyAssertion(:s :a :b)
    ClassAssertion(:D :a)"
    ));
}

/// Inverse-role domain: `Domain(r⁻, C)` constrains the **target** of an
/// `r`-edge. Exercises the `Role::flip` branch that sends `r⁻` to `r`.
#[test]
fn inverse_role_domain_fires_on_edge_target() {
    assert!(!verdict_identical(
        r"    Declaration(ObjectProperty(:r)) Declaration(Class(:C)) Declaration(Class(:D))
    Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))
    ObjectPropertyDomain(ObjectInverseOf(:r) :C)
    DisjointClasses(:C :D)
    ObjectPropertyAssertion(:r :a :b)
    ClassAssertion(:D :b)"
    ));
}

/// A **multi-disjunct** residual: `(≥1 r) ⊑ C ⊔ E`. The absorbed rule's
/// target label is the remaining `Or`, so the disjunction still has to be
/// solved — just at the source of an r-edge instead of at every node. Both
/// disjuncts excluded ⇒ INCONSISTENT.
#[test]
fn multi_disjunct_domain_rule_still_branches() {
    assert!(!verdict_identical(
        r"    Declaration(ObjectProperty(:r))
    Declaration(Class(:C)) Declaration(Class(:D)) Declaration(Class(:E))
    Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))
    SubClassOf(ObjectMinCardinality(1 :r) ObjectUnionOf(:C :E))
    DisjointClasses(:C :D)
    DisjointClasses(:E :D)
    ObjectPropertyAssertion(:r :a :b)
    ClassAssertion(:D :a)"
    ));
}

/// And its satisfiable sibling: only ONE disjunct excluded ⇒ CONSISTENT.
/// Guards against a rewrite that dropped `rest` and asserted `⊥`.
#[test]
fn multi_disjunct_domain_rule_open_alternative_stays_consistent() {
    assert!(verdict_identical(
        r"    Declaration(ObjectProperty(:r))
    Declaration(Class(:C)) Declaration(Class(:D)) Declaration(Class(:E))
    Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))
    SubClassOf(ObjectMinCardinality(1 :r) ObjectUnionOf(:C :E))
    DisjointClasses(:C :D)
    ObjectPropertyAssertion(:r :a :b)
    ClassAssertion(:D :a)"
    ));
}
