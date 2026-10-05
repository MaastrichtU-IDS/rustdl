//! The main tableau's `Asymmetric` / `DisjointObjectProperties` check (#198 item 2).
//!
//! `apply_role_axioms` compared raw forward `RoleId`s and skipped self-loops, so the
//! per-pair surface (`sat`, `subclass`, `explain`) missed clashes the wedge finds:
//! an asymmetric role's self-loop, an asymmetric symmetric role, and disjoint roles
//! reached through a common sub-role. Every test runs with the wedge OFF so the main
//! tableau is the engine answering.

#![allow(clippy::doc_markdown)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use std::io::Cursor;

// Env mutation is process-wide; serialise and restore on Drop.
static ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct EnvGuard(&'static str, Option<std::ffi::OsString>);

impl EnvGuard {
    #[allow(unsafe_code)]
    fn set(key: &'static str, value: &str) -> Self {
        let prior = std::env::var_os(key);
        // SAFETY: every mutation here happens while ENV_MUTEX is held by the caller.
        unsafe { std::env::set_var(key, value) };
        Self(key, prior)
    }
}

impl Drop for EnvGuard {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        // SAFETY: as above — the lock is still held by the test body.
        unsafe {
            match self.1.take() {
                Some(v) => std::env::set_var(self.0, v),
                None => std::env::remove_var(self.0),
            }
        }
    }
}

fn onto(body: &str) -> SetOntology<RcStr> {
    let src = format!(
        "Prefix(:=<http://ex#>)\n\
         Ontology(\n\
         Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C))\n\
         Declaration(ObjectProperty(:r)) Declaration(ObjectProperty(:s)) Declaration(ObjectProperty(:t))\n\
         {body}\n)"
    );
    let mut reader = Cursor::new(src);
    read_ofn(&mut reader, ParserConfiguration::default())
        .expect("parse")
        .0
}

/// `A` satisfiable, decided by the main tableau.
fn tableau_sat(body: &str) -> bool {
    let _lock = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _wedge = EnvGuard::set("RUSTDL_HYPERTABLEAU", "0");
    owl_dl_reasoner::is_class_satisfiable(&onto(body), "http://ex#A").expect("sat")
}

#[test]
fn an_asymmetric_self_loop_clashes() {
    assert!(!tableau_sat(
        "AsymmetricObjectProperty(:r)\nSubClassOf(:A ObjectHasSelf(:r))"
    ));
}

#[test]
fn an_asymmetric_symmetric_role_clashes() {
    assert!(!tableau_sat(
        "AsymmetricObjectProperty(:r)\nSymmetricObjectProperty(:r)\nSubClassOf(:A ObjectSomeValuesFrom(:r :B))"
    ));
}

/// `t ⊑ r`, `Asymmetric(r)`: a `t` self-loop is an `r` self-loop.
#[test]
fn asymmetry_sees_a_sub_role_self_loop() {
    assert!(!tableau_sat(
        "AsymmetricObjectProperty(:r)\nSubObjectPropertyOf(:t :r)\nSubClassOf(:A ObjectHasSelf(:t))"
    ));
}

/// `t ⊑ r`, `t ⊑ s`, `Disjoint(r, s)`: any `t`-edge is both an `r`- and an `s`-edge.
#[test]
fn disjoint_roles_see_a_common_sub_role() {
    assert!(!tableau_sat(
        "DisjointObjectProperties(:r :s)\nSubObjectPropertyOf(:t :r)\nSubObjectPropertyOf(:t :s)\n\
         SubClassOf(:A ObjectSomeValuesFrom(:t :B))"
    ));
}

/// FP guard: a one-way edge on an asymmetric role.
#[test]
fn a_one_way_asymmetric_edge_is_fine() {
    assert!(tableau_sat(
        "AsymmetricObjectProperty(:r)\nSubClassOf(:A ObjectSomeValuesFrom(:r :B))"
    ));
}

/// FP guard: an asymmetric SUB-role of a symmetric role need not be symmetric.
#[test]
fn an_asymmetric_sub_role_of_a_symmetric_role_is_fine() {
    assert!(tableau_sat(
        "AsymmetricObjectProperty(:t)\nSubObjectPropertyOf(:t :r)\nSymmetricObjectProperty(:r)\n\
         SubClassOf(:A ObjectSomeValuesFrom(:t :B))"
    ));
}

/// FP guard: separate successors on disjoint roles.
#[test]
fn separate_successors_on_disjoint_roles_are_fine() {
    assert!(tableau_sat(
        "DisjointObjectProperties(:r :s)\n\
         SubClassOf(:A ObjectIntersectionOf(ObjectSomeValuesFrom(:r :B) ObjectSomeValuesFrom(:s :B)))"
    ));
}

// ── `≤n` trial merge × role axioms (#200 review) ────────────────────────────
//
// `apply_max` keeps a merge pair when the survivor is clash-free. A merge whose
// only consequence is an asymmetry violation through the fused edges must be
// rejected inside the trial too, or the `≤`-rule commits to it without a choice
// point and a satisfiable class reads unsat (the #76 shape). Here `A` has three
// `s.C` witnesses under `≤2 s.C`: merging the `r → a` and `a → r` witnesses
// violates asymmetry, merging either with the plain `C3` witness is harmless, so
// `A` is satisfiable (HermiT, Konclude). The `C3` witness arrives last, behind two
// unfoldings, so the clashing pair is enumerated first.

const MERGE_DECLS: &str = "Declaration(Class(:C1)) Declaration(Class(:C2)) Declaration(Class(:C3))\n\
     Declaration(Class(:D1)) Declaration(Class(:D2)) Declaration(ObjectProperty(:q))\n\
     Declaration(NamedIndividual(:a))\n\
     SubClassOf(:C1 :C) SubClassOf(:C2 :C) SubClassOf(:C3 :C) ClassAssertion(:A :a)\n";

fn merge_fixture(back_edge: &str, max: u32, with_harmless: bool) -> String {
    let harmless = if with_harmless {
        "SubClassOf(:A :D1) SubClassOf(:D1 :D2) SubClassOf(:D2 ObjectSomeValuesFrom(:s :C3))"
    } else {
        ""
    };
    format!(
        "{MERGE_DECLS}AsymmetricObjectProperty(:r)\n\
         SubClassOf(:A ObjectMaxCardinality({max} :s :C))\n\
         SubClassOf(:A ObjectSomeValuesFrom(:s ObjectIntersectionOf(:C1 ObjectHasValue(:r :a))))\n\
         SubClassOf(:A ObjectSomeValuesFrom(:s ObjectIntersectionOf(:C2 {back_edge})))\n\
         {harmless}"
    )
}

const FWD_BACK: &str = "ObjectSomeValuesFrom(ObjectInverseOf(:r) ObjectOneOf(:a))";

#[test]
fn a_harmless_merge_is_found_past_an_asymmetric_one_declared_inverse() {
    let body = merge_fixture("ObjectHasValue(:q :a)", 2, true) + "\nInverseObjectProperties(:q :r)";
    assert!(tableau_sat(&body));
}

#[test]
fn a_harmless_merge_is_found_past_an_asymmetric_one_inverse_role() {
    assert!(tableau_sat(&merge_fixture(FWD_BACK, 2, true)));
}

/// Positive control: under `≤1` the clashing merge is forced, so `A` is unsat.
#[test]
fn a_forced_asymmetric_merge_is_still_unsat() {
    let body =
        merge_fixture("ObjectHasValue(:q :a)", 1, false) + "\nInverseObjectProperties(:q :r)";
    assert!(!tableau_sat(&body));
    assert!(!tableau_sat(&merge_fixture(FWD_BACK, 1, false)));
}
