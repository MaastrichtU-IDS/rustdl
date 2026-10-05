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
