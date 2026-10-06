//! A TBox-only KB made inconsistent through a named individual (#194 item 2).
//!
//! Every named individual denotes a domain element, so `{a} ⊑ ⊥` has no model,
//! and neither does `{a} ⊑ ∃r.{a}` with `r` irreflexive. The wedge consistency
//! route seeds one node per individual and catches all of these, but it was
//! only built when the KB had an `ABox` axiom: with none, `classify` and
//! `is_consistent` both reported consistent. Adding any unrelated assertion
//! made both surfaces correct, which is what located the gate.
//!
//! Every inconsistent case is confirmed by HermiT. Konclude agrees on the
//! `⊥`/`ObjectHasValue` cases and reports the cycle cases consistent, its usual
//! under-report.

#![allow(clippy::unwrap_used)]
#![allow(clippy::doc_markdown)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use std::io::Cursor;

fn onto(body: &str) -> SetOntology<RcStr> {
    let src = format!(
        "Prefix(:=<http://ex#>)\n\
         Prefix(owl:=<http://www.w3.org/2002/07/owl#>)\n\
         Ontology(\n\
         Declaration(Class(:A)) Declaration(ObjectProperty(:r))\n\
         Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))\n\
         {body}\n)"
    );
    let mut reader = Cursor::new(src);
    read_ofn(&mut reader, ParserConfiguration::default())
        .expect("parse")
        .0
}

/// `(classify says consistent, is_consistent)`.
fn verdicts(body: &str) -> (bool, bool) {
    // Every test locks: the tests below set environment variables, and a
    // reader running under `RUSTDL_WEDGE_CONSISTENCY=0` would see a different
    // engine.
    let _lock = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let o = onto(body);
    (
        !owl_dl_reasoner::classify(&o).unwrap().stats().inconsistent,
        owl_dl_reasoner::is_consistent(&o).unwrap(),
    )
}

#[test]
fn an_unsatisfiable_individual_is_inconsistent() {
    assert_eq!(
        verdicts("SubClassOf(ObjectOneOf(:a) owl:Nothing)"),
        (false, false)
    );
}

#[test]
fn an_individual_in_an_unsatisfiable_class_is_inconsistent() {
    assert_eq!(
        verdicts("SubClassOf(ObjectOneOf(:a) :A)\nSubClassOf(:A owl:Nothing)"),
        (false, false)
    );
}

#[test]
fn a_self_cycle_through_an_individual_breaks_irreflexivity() {
    assert_eq!(
        verdicts(
            "IrreflexiveObjectProperty(:r)\n\
             SubClassOf(ObjectOneOf(:a) ObjectSomeValuesFrom(:r ObjectOneOf(:a)))"
        ),
        (false, false)
    );
}

#[test]
fn a_has_value_self_cycle_breaks_irreflexivity() {
    assert_eq!(
        verdicts(
            "IrreflexiveObjectProperty(:r)\nSubClassOf(ObjectOneOf(:a) ObjectHasValue(:r :a))"
        ),
        (false, false)
    );
}

#[test]
fn a_two_cycle_through_individuals_breaks_asymmetry() {
    assert_eq!(
        verdicts(
            "AsymmetricObjectProperty(:r)\n\
             SubClassOf(ObjectOneOf(:a) ObjectSomeValuesFrom(:r ObjectOneOf(:b)))\n\
             SubClassOf(ObjectOneOf(:b) ObjectSomeValuesFrom(:r ObjectOneOf(:a)))"
        ),
        (false, false)
    );
}

/// The individual reaches the clash only through a class.
#[test]
fn a_self_cycle_through_a_class_breaks_irreflexivity() {
    assert_eq!(
        verdicts(
            "IrreflexiveObjectProperty(:r)\n\
             SubClassOf(:A ObjectHasValue(:r :a))\nSubClassOf(ObjectOneOf(:a) :A)"
        ),
        (false, false)
    );
}

/// FP guard: the self-cycle is fine without irreflexivity.
#[test]
fn a_self_cycle_alone_is_consistent() {
    assert_eq!(
        verdicts("SubClassOf(ObjectOneOf(:a) ObjectSomeValuesFrom(:r ObjectOneOf(:a)))"),
        (true, true)
    );
}

/// FP guard: a one-way edge between two individuals respects asymmetry.
#[test]
fn a_one_way_edge_respects_asymmetry() {
    assert_eq!(
        verdicts(
            "AsymmetricObjectProperty(:r)\n\
             SubClassOf(ObjectOneOf(:a) ObjectSomeValuesFrom(:r ObjectOneOf(:b)))"
        ),
        (true, true)
    );
}

/// FP guard: an edge to a different individual respects irreflexivity, since
/// `a` and `b` are not forced to be the same.
#[test]
fn an_edge_to_another_individual_respects_irreflexivity() {
    assert_eq!(
        verdicts(
            "IrreflexiveObjectProperty(:r)\n\
             SubClassOf(ObjectOneOf(:a) ObjectSomeValuesFrom(:r ObjectOneOf(:b)))"
        ),
        (true, true)
    );
}

/// FP guard: an unsatisfiable class with no individual in it is not an
/// inconsistency.
#[test]
fn an_unsatisfiable_class_without_individuals_is_consistent() {
    assert_eq!(
        verdicts("SubClassOf(:A owl:Nothing)\nSubClassOf(:A ObjectHasValue(:r :a))"),
        (true, true)
    );
}

// ---- When the wedge gives up (#204 review) ----
//
// The wedge route can stall. Its fall-through (the main tableau's `⊤` probe)
// used to seed only `ABox` individuals, so it never saw an individual named
// only in the `TBox` and answered a confident `consistent`. And `classify`'s
// pre-check read a stall as "no clash" and reported `incomplete: false`.

static ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct EnvGuard {
    key: &'static str,
    prior: Option<std::ffi::OsString>,
}

impl EnvGuard {
    #[allow(unsafe_code)]
    fn set(key: &'static str, value: &str) -> Self {
        let prior = std::env::var_os(key);
        // SAFETY: serialised by ENV_MUTEX, restored on Drop.
        unsafe { std::env::set_var(key, value) };
        Self { key, prior }
    }
}

impl Drop for EnvGuard {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        // SAFETY: see `EnvGuard::set`.
        unsafe {
            match &self.prior {
                Some(v) => std::env::set_var(self.key, v),
                None => std::env::remove_var(self.key),
            }
        }
    }
}

/// Pigeonhole over nominals: `{a}` has `n` pairwise-distinct `r`-successors,
/// each in one of `m` disjoint holes, at most one per hole. Inconsistent iff
/// `n > m` (HermiT and Konclude agree on `n = 9, m = 8`).
fn pigeonhole(n: usize, m: usize) -> String {
    use std::fmt::Write as _;
    let pigeons: Vec<String> = (0..n).map(|i| format!(":p{i}")).collect();
    let holes: Vec<String> = (0..m).map(|j| format!(":H{j}")).collect();
    let mut body = String::new();
    for p in &pigeons {
        let _ = writeln!(body, "Declaration(NamedIndividual({p}))");
    }
    for h in &holes {
        let _ = writeln!(body, "Declaration(Class({h}))");
    }
    let exists: Vec<String> = pigeons
        .iter()
        .map(|p| format!("ObjectSomeValuesFrom(:r ObjectOneOf({p}))"))
        .collect();
    let _ = writeln!(
        body,
        "SubClassOf(ObjectOneOf(:a) ObjectIntersectionOf({}))",
        exists.join(" ")
    );
    let nominals: Vec<String> = pigeons
        .iter()
        .map(|p| format!("ObjectOneOf({p})"))
        .collect();
    let _ = writeln!(body, "DisjointClasses({})", nominals.join(" "));
    let _ = writeln!(
        body,
        "SubClassOf(ObjectOneOf(:a) ObjectAllValuesFrom(:r ObjectUnionOf({})))",
        holes.join(" ")
    );
    let _ = writeln!(body, "DisjointClasses({})", holes.join(" "));
    for h in &holes {
        let _ = writeln!(
            body,
            "SubClassOf(ObjectOneOf(:a) ObjectMaxCardinality(1 :r {h}))"
        );
    }
    body
}

/// With the wedge route off, `is_consistent` is decided by the main tableau
/// alone, which must now seed `TBox`-only individuals.
#[test]
fn the_main_tableau_seeds_tbox_only_individuals() {
    let _lock = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _wedge = EnvGuard::set("RUSTDL_WEDGE_CONSISTENCY", "0");
    for body in [
        "SubClassOf(ObjectOneOf(:a) owl:Nothing)".to_owned(),
        "IrreflexiveObjectProperty(:r)\n\
         SubClassOf(ObjectOneOf(:a) ObjectSomeValuesFrom(:r ObjectOneOf(:a)))"
            .to_owned(),
        pigeonhole(5, 4),
    ] {
        assert!(
            !owl_dl_reasoner::is_consistent(&onto(&body)).unwrap(),
            "the main tableau must see `a`: {body}"
        );
    }
}

/// FP guard for the seeding: a consistent `TBox`-individual KB stays
/// consistent when the main tableau decides it.
#[test]
fn seeding_tbox_only_individuals_keeps_consistent_kbs_consistent() {
    let _lock = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _wedge = EnvGuard::set("RUSTDL_WEDGE_CONSISTENCY", "0");
    for body in [
        "AsymmetricObjectProperty(:r)\n\
         SubClassOf(ObjectOneOf(:a) ObjectSomeValuesFrom(:r ObjectOneOf(:b)))"
            .to_owned(),
        pigeonhole(4, 4),
    ] {
        assert!(
            owl_dl_reasoner::is_consistent(&onto(&body)).unwrap(),
            "{body}"
        );
    }
}

/// With a budget far too small for the wedge, `is_consistent` must not report
/// a confident `consistent` on the inconsistent pigeonhole: either it decides
/// `inconsistent` or it marks the answer incomplete.
#[test]
fn a_stalled_consistency_check_is_never_a_confident_consistent() {
    let _lock = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _budget = EnvGuard::set("RUSTDL_CONSISTENCY_FALLBACK_MS", "200");
    let (consistent, stats) =
        owl_dl_reasoner::is_consistent_with_stats(&onto(&pigeonhole(9, 8))).unwrap();
    assert!(
        !consistent || stats.incomplete,
        "confident `consistent` after a stall"
    );
}

/// The same for `classify`: a stalled pre-check is reported, so a consumer
/// can tell "no clash found in budget" from a verdict.
#[test]
fn a_stalled_classify_precheck_is_reported() {
    let _lock = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _budget = EnvGuard::set("RUSTDL_CLASSIFY_INCONSISTENCY_MS", "200");
    let c = owl_dl_reasoner::classify(&onto(&pigeonhole(9, 8))).unwrap();
    let stats = c.stats();
    assert!(
        stats.inconsistent || stats.consistency_undetermined,
        "classify read a stalled pre-check as a verdict"
    );
}

/// Control: a pre-check that finishes does not raise the flag.
#[test]
fn a_finished_classify_precheck_is_not_flagged() {
    let _lock = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let c = owl_dl_reasoner::classify(&onto(
        "SubClassOf(ObjectOneOf(:a) ObjectSomeValuesFrom(:r ObjectOneOf(:b)))",
    ))
    .unwrap();
    assert!(!c.stats().inconsistent);
    assert!(!c.stats().consistency_undetermined);
}
