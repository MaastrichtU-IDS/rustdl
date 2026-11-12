//! #510 (C): `InstanceFlags::depth_cap_hit` says that a deadline or the node cap cut an
//! instance probe after it had hit the tableau's decision-depth limit at least once.
//! It is the depth-bound probe that `depth_capped` misses: early abandon off, or the
//! deadline arriving before early abandon's 32nd hit (a loaded or slow host).
//!
//! Fixtures:
//! - `X ⊑ ⊓_{k<280} (∀r_k.A ⊔ ∀s_k.A)`, `a : X` (the #510 follow-up's): every branch of
//!   `{a} ⊓ ¬Y` hits the cap at 256 levels. With early abandon armed but never latching
//!   (`RUSTDL_TABLEAU_EARLY_ABANDON_HITS=1000000`) or off, a 3 s deadline cuts it after
//!   tens of thousands of hits; the first comes within well under a second.
//! - A pigeonhole forced under `¬Y` (9 pigeons, 8 holes): its decision depth is at most
//!   9, so a deadline cuts it with no hit at all, whatever the host's speed.
//!
//! The node-cap arm is pinned by the CLI test (`instance_depth_cut_cli.rs`), because
//! the node cap is read once per process. The settings here are read per call, so the
//! tests share one process under an env mutex.

#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::{InstanceFlags, instances_of_reporting, is_instance_of_reporting};
use std::fmt::Write as _;
use std::io::Cursor;

const Y: &str = "http://example.org/d#Y";
const A: &str = "http://example.org/d#a";

fn parse(src: &str) -> SetOntology<RcStr> {
    read_ofn(&mut Cursor::new(src), ParserConfiguration::default())
        .unwrap()
        .0
}

fn forall_unions() -> SetOntology<RcStr> {
    let mut src = String::from(
        "Prefix(:=<http://example.org/d#>)\nOntology(<http://example.org/d>\n\
         Declaration(NamedIndividual(:a)) Declaration(Class(:Y)) Declaration(Class(:A))\n\
         SubClassOf(:X ObjectIntersectionOf(",
    );
    for k in 0..280 {
        write!(
            src,
            " ObjectUnionOf(ObjectAllValuesFrom(:r{k} :A) ObjectAllValuesFrom(:s{k} :A))"
        )
        .unwrap();
    }
    src.push_str("))\nClassAssertion(:X :a)\n)\n");
    parse(&src)
}

fn pigeonhole() -> SetOntology<RcStr> {
    const HOLES: usize = 8;
    let p = |i: usize, j: usize| format!(":P{i}_{j}");
    let holes = |i: usize| (0..HOLES).map(|j| p(i, j)).collect::<Vec<_>>().join(" ");
    let mut src = String::from(
        "Prefix(owl:=<http://www.w3.org/2002/07/owl#>)\nPrefix(:=<http://example.org/d#>)\n\
         Ontology(<http://example.org/d>\n\
         Declaration(NamedIndividual(:a)) Declaration(Class(:Y)) Declaration(Class(:X))\n",
    );
    for i in 0..HOLES {
        writeln!(src, "SubClassOf(:X ObjectUnionOf({}))", holes(i)).unwrap();
    }
    writeln!(
        src,
        "SubClassOf(owl:Thing ObjectUnionOf(:Y {}))",
        holes(HOLES)
    )
    .unwrap();
    for j in 0..HOLES {
        for i in 0..=HOLES {
            for k in i + 1..=HOLES {
                writeln!(src, "DisjointClasses({} {})", p(i, j), p(k, j)).unwrap();
            }
        }
    }
    src.push_str("ClassAssertion(:X :a)\n)\n");
    parse(&src)
}

static ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn lock() -> std::sync::MutexGuard<'static, ()> {
    ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Sets env vars for the guard's lifetime and restores them on drop.
struct SetEnv(Vec<(&'static str, Option<std::ffi::OsString>)>);

impl SetEnv {
    #[allow(unsafe_code)]
    fn new(vars: &[(&'static str, &str)]) -> Self {
        let mut prior = Vec::new();
        for &(key, value) in vars {
            prior.push((key, std::env::var_os(key)));
            // SAFETY: serialized by ENV_MUTEX, restored on drop.
            unsafe { std::env::set_var(key, value) };
        }
        Self(prior)
    }
}

impl Drop for SetEnv {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        for (key, value) in self.0.drain(..).rev() {
            // SAFETY: see `new`.
            unsafe {
                match value {
                    Some(v) => std::env::set_var(key, v),
                    None => std::env::remove_var(key),
                }
            }
        }
    }
}

fn instance(o: &SetOntology<RcStr>, env: &[(&'static str, &str)]) -> (bool, InstanceFlags) {
    let _e = SetEnv::new(env);
    is_instance_of_reporting(o, Y, A).unwrap()
}

fn instances(o: &SetOntology<RcStr>, env: &[(&'static str, &str)]) -> InstanceFlags {
    let _e = SetEnv::new(env);
    instances_of_reporting(o, Y).unwrap().1
}

const ARMED_NOT_LATCHED: [(&str, &str); 2] = [
    ("RUSTDL_TABLEAU_EARLY_ABANDON_HITS", "1000000"),
    ("RUSTDL_REALIZE_PAIR_TIMEOUT_MS", "3000"),
];
const EARLY_ABANDON_OFF: [(&str, &str); 2] = [
    ("RUSTDL_TABLEAU_EARLY_ABANDON", "0"),
    ("RUSTDL_REALIZE_PAIR_TIMEOUT_MS", "3000"),
];

#[test]
fn a_budget_cut_after_a_depth_cap_hit_is_a_depth_cap_hit() {
    let _l = lock();
    let o = forall_unions();
    for (case, env) in [
        ("armed, not latched", &ARMED_NOT_LATCHED),
        ("early abandon off", &EARLY_ABANDON_OFF),
    ] {
        let (holds, f) = instance(&o, env);
        assert!(!holds, "[{case}] {f:?}");
        assert!(f.incomplete && f.depth_cap_hit, "[{case}] {f:?}");
        assert!(
            !f.depth_capped && !f.no_verdict,
            "[{case}] one cause: {f:?}"
        );
        let f = instances(&o, env);
        assert!(
            f.incomplete && f.depth_cap_hit && !f.depth_capped,
            "[{case}] {f:?}"
        );
    }
}

#[test]
fn a_depth_cut_is_not_a_depth_cap_hit_and_the_deadline_free_probe_reports_neither() {
    let _l = lock();
    let o = forall_unions();
    // Early abandon at its default ends the probe on its 32nd hit: `depth_capped`.
    let (_, f) = instance(&o, &[("RUSTDL_REALIZE_PAIR_TIMEOUT_MS", "60000")]);
    assert!(f.depth_capped && !f.depth_cap_hit, "{f:?}");
    let (holds, f) = instance(&o, &[("RUSTDL_REALIZE_PAIR_TIMEOUT_MS", "0")]);
    assert!(
        !holds && !f.incomplete && !f.depth_capped && !f.depth_cap_hit,
        "{f:?}"
    );
}

#[test]
fn a_budget_cut_with_no_depth_cap_hit_reports_neither() {
    let _l = lock();
    let o = pigeonhole();
    // The default 750 ms deadline cuts it; it never comes near the depth cap.
    let (holds, f) = instance(&o, &[]);
    assert!(!holds && f.incomplete, "{f:?}");
    assert!(
        !f.depth_capped && !f.depth_cap_hit && !f.no_verdict,
        "{f:?}"
    );
}
