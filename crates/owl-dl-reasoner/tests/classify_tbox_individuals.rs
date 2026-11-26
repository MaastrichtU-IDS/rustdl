//! #608: classify ignores what the `TBox` says about an individual its model never
//! reaches.
//!
//! Every named individual exists in every model. A `TBox` axiom about `b`
//! (`{b} ⊑ ∃S.{a}`) therefore constrains `a`, and through `a` every class whose model
//! reaches `{a}`. Classify's probes never seeded `b`: the main tableau's
//! `decide_classify*` seed carried only the `ABox`, and the wedge starts from the
//! probe root alone. A `{b}`-antecedent clause then never fires, and on each positive
//! fixture below `subclass` answered `yes` while the pair was missing from the
//! closure, with nothing reported.
//!
//! `RUSTDL_CLASSIFY_TBOX_INDIVIDUALS=1` seeds the individuals named only in `TBox`
//! nominals in both probes, as #356 did for the per-query probes. Seeding only
//! restricts a probe to genuine models, so the controls (the same shapes with the
//! forcing edge pointing at `c` instead of `a`) must not change.
//!
//! Every expected closure is adjudicated by `HermiT` and Konclude (fixtures and oracle
//! runs in `~/rustdl/sweeps/bugfix/608/oracle/` and `oracle2/`). They agree on all but `t3`, where
//! Konclude reports `X` satisfiable. `HermiT` and the semantics say unsat: `b S a` holds
//! in every model, so `a`, the `R`-successor of every `X`, has an `S`-predecessor. The
//! seeded per-query probe (#356) agrees: `rustdl sat t3 X` is `unsat`, and `sat` with
//! `RUSTDL_QUERY_TBOX_INDIVIDUALS=0` gives Konclude's answer.
//! Negatives first. Closures are compared at a non-truncating budget, so a timing cut
//! cannot pass or fail a test.
#![allow(clippy::unwrap_used)]

use std::io::Cursor;
use std::time::Duration;

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::{Classification, classify_with_timeout};

// Tests set `RUSTDL_CLASSIFY_TBOX_INDIVIDUALS` / `RUSTDL_HYPERTABLEAU`; every test
// locks, so none reads them mid-change.
static ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

const EX: &str = "http://ex/#";
const SEED: (&str, &str) = ("RUSTDL_CLASSIFY_TBOX_INDIVIDUALS", "1");
const NO_WEDGE: (&str, &str) = ("RUSTDL_HYPERTABLEAU", "0");

const DECLS: &str = "Declaration(Class(:X)) Declaration(Class(:D)) Declaration(Class(:E)) \
     Declaration(Class(:C))\n\
     Declaration(ObjectProperty(:R)) Declaration(ObjectProperty(:S))\n\
     Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b)) \
     Declaration(NamedIndividual(:c))";
const NAMES: &[&str] = &["X", "D", "E", "C"];

/// Every fixture's class `X` reaches `a`.
const X_TO_A: &str = "SubClassOf(:X ObjectSomeValuesFrom(:R ObjectOneOf(:a)))";
/// `X ⊑ D` once `a` has an `S`-predecessor.
const R_S_INV_D: &str =
    "SubClassOf(ObjectSomeValuesFrom(:R ObjectSomeValuesFrom(ObjectInverseOf(:S) owl:Thing)) :D)";

/// The issue's repro: `{b} ⊑ ∃S.{a}`.
fn t1(to: &str) -> String {
    format!(
        "{X_TO_A} SubClassOf(ObjectOneOf(:b) ObjectSomeValuesFrom(:S ObjectOneOf(:{to}))) {R_S_INV_D}"
    )
}
/// `b`'s edge as `ObjectHasValue`, and a universal on `b` that labels `a`.
fn t2(to: &str) -> String {
    format!(
        "{X_TO_A} SubClassOf(ObjectOneOf(:b) ObjectHasValue(:S :{to})) \
         SubClassOf(ObjectOneOf(:b) ObjectAllValuesFrom(:S :E)) \
         SubClassOf(ObjectSomeValuesFrom(:R :E) :D)"
    )
}
/// An unsat class: `X` forbids `a` any `S`-predecessor, and `b` is one.
fn t3(to: &str) -> String {
    format!(
        "{X_TO_A} SubClassOf(ObjectOneOf(:b) ObjectSomeValuesFrom(:S ObjectOneOf(:{to}))) \
         SubClassOf(:X ObjectAllValuesFrom(:R ObjectMaxCardinality(0 ObjectInverseOf(:S) owl:Thing)))"
    )
}
/// `b` through an enumeration class: `C ≡ {b}`, `C ⊑ ∃S.{a}`.
fn t4(to: &str) -> String {
    format!(
        "{X_TO_A} EquivalentClasses(:C ObjectOneOf(:b)) \
         SubClassOf(:C ObjectSomeValuesFrom(:S ObjectOneOf(:{to}))) {R_S_INV_D}"
    )
}

/// `b` is in the `ABox`, but only in `DifferentIndividuals`, which gives the wedge no
/// `{b}` fact: the wedge must seed `b` as if it were named only in the `TBox`.
fn d1(to: &str) -> String {
    format!("{} DifferentIndividuals(:a :b)", t1(to))
}
/// `X ⊑ {b}`: the probe root merges into the seeded `b` node.
const T5: &str = "SubClassOf(:X ObjectOneOf(:b)) \
     SubClassOf(ObjectOneOf(:b) ObjectSomeValuesFrom(:S ObjectOneOf(:a))) \
     SubClassOf(ObjectSomeValuesFrom(:S ObjectOneOf(:a)) :D) \
     SubClassOf(:E ObjectSomeValuesFrom(:R ObjectOneOf(:a)))";
/// `X` reaches `a` or `c`; `b` has an `S`-edge to `a`, and (`both`) to `c`. Only with
/// both edges does every branch force `X ⊑ D`.
fn t6(both: bool) -> String {
    let to_c = if both {
        "SubClassOf(ObjectOneOf(:b) ObjectSomeValuesFrom(:S ObjectOneOf(:c)))"
    } else {
        ""
    };
    format!(
        "SubClassOf(:X ObjectSomeValuesFrom(:R ObjectOneOf(:a :c))) \
         SubClassOf(ObjectOneOf(:b) ObjectSomeValuesFrom(:S ObjectOneOf(:a))) {to_c} {R_S_INV_D}"
    )
}
/// An `ABox` individual (`c`) beside the `TBox`-only `b`.
fn t7(to: &str) -> String {
    format!("{} ClassAssertion(:E :c)", t1(to))
}
/// `b` occurs only in a clause body: nothing derives a nominal, so no wedge node can
/// carry one, and an unseeded wedge misses nothing (`b`'s `S`-successor need not be
/// `X`'s `R`-successor).
const T8: &str = "SubClassOf(ObjectOneOf(:b) ObjectSomeValuesFrom(:S :E)) \
     SubClassOf(:X ObjectSomeValuesFrom(:R :E)) \
     SubClassOf(ObjectSomeValuesFrom(:R ObjectSomeValuesFrom(ObjectInverseOf(:S) owl:Thing)) :D)";

fn onto(body: &str) -> SetOntology<RcStr> {
    let src = format!(
        "Prefix(:=<{EX}>)\nPrefix(owl:=<http://www.w3.org/2002/07/owl#>)\n\
         Ontology(<http://ex/o>\n{DECLS}\n{body}\n)\n"
    );
    read_ofn(&mut Cursor::new(src), ParserConfiguration::default())
        .unwrap()
        .0
}

fn iri(c: &str) -> String {
    format!("{EX}{c}")
}

struct EnvGuard<'a>(&'a [(&'a str, &'a str)]);

impl Drop for EnvGuard<'_> {
    fn drop(&mut self) {
        for (k, _) in self.0 {
            // SAFETY: dropped while the test still holds `ENV_MUTEX`.
            #[allow(unsafe_code)]
            unsafe {
                std::env::remove_var(k);
            }
        }
    }
}

fn run(body: &str, env: &[(&str, &str)]) -> Classification {
    let _g = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    // Declared after `_g`, so it drops (and unsets) before the lock is released.
    let _env = EnvGuard(env);
    // SAFETY: every test in this binary holds `ENV_MUTEX` while the variables are set.
    for (k, v) in env {
        #[allow(unsafe_code)]
        unsafe {
            std::env::set_var(k, v);
        }
    }
    classify_with_timeout(&onto(body), Duration::from_secs(2)).expect("classify")
}

/// The closure over [`NAMES`], an unsat class as `(name, "Nothing")`.
fn closure(c: &Classification) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for &s in NAMES {
        if c.unsatisfiable_classes().iter().any(|u| *u == iri(s)) {
            out.push((s.to_owned(), "Nothing".to_owned()));
            continue;
        }
        for &d in NAMES {
            if s != d && c.is_subclass(&iri(s), &iri(d)) {
                out.push((s.to_owned(), d.to_owned()));
            }
        }
    }
    out.sort();
    out
}

fn pairs(p: &[(&str, &str)]) -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = p
        .iter()
        .map(|(a, b)| ((*a).to_owned(), (*b).to_owned()))
        .collect();
    v.sort();
    v
}

/// With the seed, through the default classifier and through the main tableau alone,
/// the closure is the oracle's. Without it, it is a subset of the oracle (sound), and
/// `tbox_individuals_unseeded` says so.
fn check(body: &str, oracle: &[(&str, &str)]) {
    let want = pairs(oracle);
    let default = run(body, &[]);
    assert!(
        default.stats().tbox_individuals_unseeded,
        "the default reports the unseeded individual"
    );
    let without = closure(&default);
    assert!(
        without.iter().all(|p| want.contains(p)),
        "unsound without the seed: {without:?} vs oracle {want:?}"
    );
    let seeded = run(body, &[SEED]);
    assert!(
        !seeded.stats().tbox_individuals_unseeded,
        "seeded, no signal"
    );
    assert_eq!(closure(&seeded), want, "seeded, default classifier");
    assert_eq!(
        closure(&run(body, &[SEED, NO_WEDGE])),
        want,
        "seeded, main tableau alone"
    );
}

// Negatives: the forcing edge goes to `c`, which no class reaches.

#[test]
fn t1_control_an_edge_to_an_unreached_individual_forces_nothing() {
    check(&t1("c"), &[]);
}

#[test]
fn t2_control_an_edge_to_an_unreached_individual_forces_nothing() {
    check(&t2("c"), &[]);
}

#[test]
fn t3_control_an_edge_to_an_unreached_individual_forces_nothing() {
    check(&t3("c"), &[]);
}

#[test]
fn t4_control_an_edge_to_an_unreached_individual_forces_nothing() {
    check(&t4("c"), &[]);
}

// Positives.

#[test]
fn t1_an_existential_on_b_reaches_a() {
    check(&t1("a"), &[("X", "D")]);
}

#[test]
fn t2_a_has_value_and_a_universal_on_b_label_a() {
    check(&t2("a"), &[("X", "D")]);
}

#[test]
fn t3_an_edge_from_b_makes_a_class_unsat() {
    check(&t3("a"), &[("X", "Nothing")]);
}

#[test]
fn t4_b_through_an_enumeration_class() {
    check(&t4("a"), &[("X", "D")]);
}

/// The gap this flag closes, at the default (flag off): pinned so a default flip
/// shows up here and is made on purpose.
#[test]
fn the_default_still_misses_the_issue_repro() {
    let c = run(&t1("a"), &[]);
    assert_eq!(closure(&c), pairs(&[]));
    assert!(c.stats().tbox_individuals_unseeded);
    assert_eq!(c.stats().tbox_individuals_unseeded_count, 2, "`a` and `b`");
    assert!(!c.completeness_guaranteed());
}

// From the PR review: the wedge's own seed set, the merge into a seeded node, a
// disjunction over seeded edges, a mixed `ABox`, and a nominal no head derives.

#[test]
fn d1_control_different_individuals_only() {
    check(&d1("c"), &[]);
}

#[test]
fn d1_an_individual_only_in_different_individuals_is_seeded_in_the_wedge() {
    check(&d1("a"), &[("X", "D")]);
}

#[test]
fn t5_the_root_merges_into_a_seeded_individual() {
    check(T5, &[("X", "D")]);
}

#[test]
fn t6_control_one_branch_unforced() {
    check(&t6(false), &[]);
}

#[test]
fn t6_a_disjunction_over_two_seeded_edges() {
    check(&t6(true), &[("X", "D")]);
}

#[test]
fn t7_control_an_abox_individual_beside_a_tbox_only_one() {
    check(&t7("c"), &[]);
}

#[test]
fn t7_an_abox_individual_beside_a_tbox_only_one() {
    check(&t7("a"), &[("X", "D")]);
}

/// The main tableau still leaves `b` out at the default (signal set); with the flag
/// the wedge has nothing to seed and needs nothing, so the signal clears.
#[test]
fn t8_a_nominal_in_a_body_only_needs_no_wedge_seed() {
    check(T8, &[]);
}

/// A wedge whose nominal semantics are off (`RUSTDL_CLASSIFY_WEDGE_NOMINALS=0`) cannot
/// seed `b`, so the signal stays set even with the flag.
#[test]
fn a_wedge_without_nominal_semantics_keeps_the_signal() {
    let c = run(&t1("a"), &[SEED, ("RUSTDL_CLASSIFY_WEDGE_NOMINALS", "0")]);
    assert!(c.stats().tbox_individuals_unseeded);
    assert!(!c.completeness_guaranteed());
}

/// Every individual has a class assertion, so none is unseeded; the wedge, which still
/// does not instantiate `b`, is reported by #533's `wedge_abox_blind` instead, or
/// finds the pair anyway.
#[test]
fn asserted_individuals_are_left_to_wedge_abox_blind() {
    let body = format!(
        "{} ClassAssertion(owl:Thing :b) ClassAssertion(owl:Thing :a)",
        t1("a")
    );
    let c = run(&body, &[]);
    assert!(!c.stats().tbox_individuals_unseeded);
    assert_eq!(c.stats().tbox_individuals_unseeded_count, 0);
    let got = closure(&c);
    assert!(
        got == pairs(&[("X", "D")]) || c.stats().wedge_abox_blind,
        "a miss must be signalled: {got:?}"
    );
    assert_eq!(
        closure(&run(&body, &[NO_WEDGE])),
        pairs(&[("X", "D")]),
        "the main tableau finds it"
    );
}
