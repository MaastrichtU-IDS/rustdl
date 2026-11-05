//! #428: the named class queries (`is_class_satisfiable*`, `is_subclass_of*`, so
//! `sat`, `subclass` and `explain`) run the #313 inconsistency guard first and,
//! after a positive answer ("unsatisfiable" / "subsumed"), the #369 lazy check,
//! as `sat-expr` / `subclass-expr` do. A witnessed inconsistency is
//! `Err(Inconsistent)`; an undetermined one keeps the answer and sets
//! `QueryStats::consistency_undetermined`.
//!
//! Fixtures:
//! - `ABOX_CLASH`: `a : A`, `a : B`, `A ⊓ B ⊑ ⊥` — the `ABox` saturation (the #313
//!   guard) finds it;
//! - `P2`: #419's `T2` (`{b} ⊑ ≥2 r.⊤`, `r` functional): inconsistent, and only the
//!   wedge (the #369 lazy check) finds it;
//! - `ctl()`: `P2` with `≥1`, consistent. Both also carry `Y ⊑ W` and `N ⊑ ⊥`.
//!
//! Every test locks the env mutex.

#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::justify::{Entailment, find_one_justification};
use owl_dl_reasoner::{
    QueryStats, ReasonError, assume_query_consistency, is_class_satisfiable,
    is_class_satisfiable_with_stats, is_subclass_of, is_subclass_of_with_stats,
};
use std::io::Cursor;

static ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn lock() -> std::sync::MutexGuard<'static, ()> {
    ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

struct SetEnvGuard {
    key: &'static str,
    prior: Option<std::ffi::OsString>,
}
impl SetEnvGuard {
    #[allow(unsafe_code)]
    fn set(key: &'static str, value: &str) -> Self {
        let prior = std::env::var_os(key);
        // SAFETY: set_var is unsafe under edition 2024. Serialized via ENV_MUTEX,
        // restored on Drop.
        unsafe { std::env::set_var(key, value) };
        Self { key, prior }
    }
}
impl Drop for SetEnvGuard {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        // SAFETY: see `set`.
        unsafe {
            match &self.prior {
                Some(v) => std::env::set_var(self.key, v),
                None => std::env::remove_var(self.key),
            }
        }
    }
}

const ABOX_CLASH: &str = r"Prefix(:=<http://ex/#>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Ontology(<http://ex/aboxclash>
  Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:X)) Declaration(Class(:Z))
  Declaration(NamedIndividual(:a))
  DisjointClasses(:A :B) ClassAssertion(:A :a) ClassAssertion(:B :a)
)";

const P2: &str = r"Prefix(:=<http://ex/#>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Ontology(<http://ex/p2>
  Declaration(Class(:X)) Declaration(Class(:Y)) Declaration(ObjectProperty(:r))
  Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))
  SubClassOf(ObjectOneOf(:b) ObjectMinCardinality(MIN :r owl:Thing))
  FunctionalObjectProperty(:r)
  Declaration(Class(:W)) Declaration(Class(:N))
  ClassAssertion(:Y :a) SubClassOf(:Y :W) SubClassOf(:N owl:Nothing)
)";

const X: &str = "http://ex/#X";
const Y: &str = "http://ex/#Y";
const Z: &str = "http://ex/#Z";
const W: &str = "http://ex/#W";
const N: &str = "http://ex/#N";

fn parse(src: &str) -> SetOntology<RcStr> {
    read_ofn(&mut Cursor::new(src), ParserConfiguration::default())
        .unwrap()
        .0
}

fn p2() -> SetOntology<RcStr> {
    parse(&P2.replace("MIN", "2"))
}

fn ctl() -> SetOntology<RcStr> {
    parse(&P2.replace("MIN", "1"))
}

fn is_inconsistent<T: std::fmt::Debug>(r: &Result<T, ReasonError>) -> bool {
    matches!(r, Err(ReasonError::Inconsistent))
}

type Answer = Result<(bool, QueryStats), ReasonError>;

/// `sat X` (as "unsatisfiable": positive) and `subclass X Z`, through every
/// public form.
fn queries(o: &SetOntology<RcStr>) -> Vec<(&'static str, Answer)> {
    vec![
        (
            "is_class_satisfiable_with_stats",
            is_class_satisfiable_with_stats(o, X).map(|(s, st)| (!s, st)),
        ),
        (
            "is_class_satisfiable",
            is_class_satisfiable(o, X).map(|s| (!s, QueryStats::default())),
        ),
        (
            "is_subclass_of_with_stats",
            is_subclass_of_with_stats(o, X, Z),
        ),
        (
            "is_subclass_of",
            is_subclass_of(o, X, Z).map(|h| (h, QueryStats::default())),
        ),
    ]
}

/// The #313 guard: an `ABox` clash errors on every form; off, the vacuous
/// answers come back.
#[test]
fn an_abox_clash_errors_inconsistent() {
    let _g = lock();
    let o = parse(ABOX_CLASH);
    for (name, r) in queries(&o) {
        assert!(is_inconsistent(&r), "{name}: {r:?}");
    }
    // With the wedge off the lazy check cannot see the clash: the guard alone.
    {
        let _wedge = SetEnvGuard::set("RUSTDL_WEDGE_CONSISTENCY", "0");
        for (name, r) in queries(&o) {
            assert!(is_inconsistent(&r), "wedge off, {name}: {r:?}");
        }
    }
    let _off = SetEnvGuard::set("RUSTDL_CLASS_QUERY_GUARD", "0");
    for (name, r) in queries(&o) {
        let (positive, st) = r.unwrap();
        assert!(positive && !st.consistency_undetermined, "{name}");
    }
}

/// An unknown class is reported as such, before the guard runs.
#[test]
fn an_unknown_class_is_reported_before_the_guard() {
    let _g = lock();
    let o = parse(ABOX_CLASH);
    let nope = "http://ex/#Nope";
    assert!(matches!(
        is_class_satisfiable_with_stats(&o, nope),
        Err(ReasonError::UnknownClass(_))
    ));
    assert!(matches!(
        is_subclass_of_with_stats(&o, X, nope),
        Err(ReasonError::UnknownClass(_))
    ));
}

/// The #369 lazy check: a wedge-only inconsistency errors after a positive
/// answer; a consistent KB keeps its answers with no flag.
#[test]
fn a_wedge_only_inconsistency_errors_after_a_positive_answer() {
    let _g = lock();
    let o = p2();
    let r = is_class_satisfiable_with_stats(&o, Y);
    assert!(is_inconsistent(&r), "sat Y: {r:?}");
    let r = is_subclass_of_with_stats(&o, Y, X);
    assert!(is_inconsistent(&r), "subclass Y X: {r:?}");
    let c = ctl();
    let (sat, st) = is_class_satisfiable_with_stats(&c, Y).unwrap();
    assert!(sat && !st.consistency_undetermined);
    let (holds, st) = is_subclass_of_with_stats(&c, Y, W).unwrap();
    assert!(holds && !st.consistency_undetermined);
    let (sat, st) = is_class_satisfiable_with_stats(&c, N).unwrap();
    assert!(!sat && !st.consistency_undetermined);
}

/// With the wedge off the verdict is undetermined: a positive answer is kept and
/// flagged; a negative one never runs the check; `C ⊑ C` is never vacuous.
#[test]
fn an_undetermined_wedge_flags_only_a_non_trivial_positive_answer() {
    let _g = lock();
    let _wedge = SetEnvGuard::set("RUSTDL_WEDGE_CONSISTENCY", "0");
    let c = ctl();
    let (sat, st) = is_class_satisfiable_with_stats(&c, N).unwrap();
    assert!(!sat && st.consistency_undetermined, "unsat N");
    let (holds, st) = is_subclass_of_with_stats(&c, Y, W).unwrap();
    assert!(holds && st.consistency_undetermined, "Y ⊑ W");
    let (sat, st) = is_class_satisfiable_with_stats(&c, Y).unwrap();
    assert!(sat && !st.consistency_undetermined, "sat Y");
    let (holds, st) = is_subclass_of_with_stats(&c, Y, X).unwrap();
    assert!(!holds && !st.consistency_undetermined, "Y ⋢ X");
    let (holds, st) = is_subclass_of_with_stats(&c, Y, Y).unwrap();
    assert!(holds && !st.consistency_undetermined, "Y ⊑ Y");
}

/// A lent "consistent" verdict (justify lends its ontology's to the subsets it
/// probes) skips the guard: an answer comes back, not `Inconsistent`.
#[test]
fn a_lent_consistent_verdict_skips_the_guard() {
    let _g = lock();
    let o = parse(ABOX_CLASH);
    let _lent = assume_query_consistency(false);
    assert!(is_class_satisfiable_with_stats(&o, X).is_ok());
    assert!(is_subclass_of_with_stats(&o, X, Z).is_ok());
}

/// justify maps the guard's `Inconsistent` to "entailed", as for instance
/// queries: the justification of `X ⊑ Z` on an inconsistent KB is its
/// inconsistency core.
#[test]
fn justify_returns_the_inconsistency_core() {
    let _g = lock();
    let o = parse(ABOX_CLASH);
    for q in [
        Entailment::SubClassOf {
            sub: X.to_owned(),
            sup: Z.to_owned(),
        },
        Entailment::Unsatisfiable {
            class: X.to_owned(),
        },
    ] {
        let j = find_one_justification(&o, &q).unwrap().expect("entailed");
        assert_eq!(j.axioms.len(), 3, "{q:?}: {:?}", j.axioms);
    }
}

/// The guard's `ABox` budget rule (#428, option (b)): unbounded in classify's
/// low-work class (`family.ofn` needs 6–12 s there, and a cut lets the query
/// stall), 3 s in the high-work class (where the six measured runaways sit).
#[test]
fn the_guard_abox_budget_is_unbounded_for_low_work_and_3s_for_high_work() {
    use owl_dl_reasoner::{
        AboxCostPredictors, INCONSISTENCY_STINGY_MS, INCONSISTENCY_WORK_THRESHOLD,
        query_guard_abox_budget_rule_ms,
    };
    let at = |edges: u64| {
        query_guard_abox_budget_rule_ms(AboxCostPredictors {
            asserted_edges: usize::try_from(edges).unwrap(),
            multiplying_rules: 1,
        })
    };
    // family.ofn: 1,337 asserted edges, work 50,806.
    let family = AboxCostPredictors {
        asserted_edges: 1337,
        multiplying_rules: 38,
    };
    assert_eq!(
        query_guard_abox_budget_rule_ms(family),
        0,
        "family: unbounded"
    );
    // ore_ont_15846: 78,441 edges x 55 multiplying rules.
    let runaway = AboxCostPredictors {
        asserted_edges: 78_441,
        multiplying_rules: 55,
    };
    assert_eq!(
        query_guard_abox_budget_rule_ms(runaway),
        INCONSISTENCY_STINGY_MS
    );
    assert_eq!(INCONSISTENCY_STINGY_MS, 3000);
    assert_eq!(at(INCONSISTENCY_WORK_THRESHOLD), 0);
    assert_eq!(
        at(INCONSISTENCY_WORK_THRESHOLD + 1),
        INCONSISTENCY_STINGY_MS
    );
}

/// `RUSTDL_QUERY_GUARD_ABOX_MS` overrides the rule, `0` included.
#[test]
fn the_guard_abox_budget_override_wins() {
    let _g = lock();
    let internal = owl_dl_core::convert::convert_ontology(&parse(ABOX_CLASH)).unwrap();
    assert_eq!(owl_dl_reasoner::query_guard_abox_budget_ms(&internal), 0);
    let _o = SetEnvGuard::set("RUSTDL_QUERY_GUARD_ABOX_MS", "7");
    assert_eq!(owl_dl_reasoner::query_guard_abox_budget_ms(&internal), 7);
}

/// On a high-work `ABox` (3 s by the rule) the override still wins, and `0` means
/// unbounded rather than "use the rule". 6,001 asserted edges × 50 transitive roles
/// is a work proxy of 300,050, just over the threshold.
#[test]
fn a_zero_override_unbounds_a_high_work_abox() {
    use std::fmt::Write as _;
    let _g = lock();
    let mut src = String::from("Prefix(:=<http://ex/#>)\nOntology(<http://ex/hw>\n");
    for i in 0..50 {
        writeln!(
            src,
            "Declaration(ObjectProperty(:t{i})) TransitiveObjectProperty(:t{i})"
        )
        .unwrap();
    }
    for i in 0..6001 {
        writeln!(src, "ObjectPropertyAssertion(:t0 :a{i} :a{})", i + 1).unwrap();
    }
    src.push(')');
    let internal = owl_dl_core::convert::convert_ontology(&parse(&src)).unwrap();
    assert_eq!(
        owl_dl_reasoner::query_guard_abox_budget_ms(&internal),
        owl_dl_reasoner::INCONSISTENCY_STINGY_MS,
        "precondition: the rule bounds this ABox"
    );
    let _o = SetEnvGuard::set("RUSTDL_QUERY_GUARD_ABOX_MS", "0");
    assert_eq!(owl_dl_reasoner::query_guard_abox_budget_ms(&internal), 0);
}

const THING: &str = "http://www.w3.org/2002/07/owl#Thing";
const NOTHING: &str = "http://www.w3.org/2002/07/owl#Nothing";

/// `sat owl:Thing` and `owl:Thing ⊑ owl:Nothing` ARE the consistency check
/// (#427): on an inconsistent KB they answer "unsat" / "yes", never an error.
/// Nothing else is exempt from the guard: `sat owl:Nothing`, `⊤ ⊑ C`, and the
/// tautologies `C ⊑ ⊤` and `⊥ ⊑ C` error, as their `sat-expr` / `subclass-expr`
/// forms do.
#[test]
fn the_consistency_check_is_exempt_and_nothing_else_is() {
    let _g = lock();
    let o = parse(ABOX_CLASH);
    let (sat, _) = is_class_satisfiable_with_stats(&o, THING).unwrap();
    assert!(!sat, "sat owl:Thing is the consistency check");
    let (holds, _) = is_subclass_of_with_stats(&o, THING, NOTHING).unwrap();
    assert!(holds, "owl:Thing ⊑ owl:Nothing is the consistency check");
    let r = is_class_satisfiable_with_stats(&o, NOTHING);
    assert!(is_inconsistent(&r), "sat owl:Nothing: {r:?}");
    let r = is_subclass_of_with_stats(&o, THING, X);
    assert!(is_inconsistent(&r), "owl:Thing ⊑ X: {r:?}");
    // The tautologies are exempt from the lazy check only, not from the guard: on a
    // proved inconsistency they error like every other named query, as
    // `sat-expr` / `subclass-expr` do. (`prove` answers them structurally, before its
    // guard: a tautology's proof uses no axiom.)
    let r = is_subclass_of_with_stats(&o, X, THING);
    assert!(is_inconsistent(&r), "X ⊑ owl:Thing: {r:?}");
    let r = is_subclass_of_with_stats(&o, NOTHING, X);
    assert!(is_inconsistent(&r), "owl:Nothing ⊑ X: {r:?}");
    // Consistent control: `⊤ ⊑ Y` is a plain "no", unflagged.
    let (holds, st) = is_subclass_of_with_stats(&ctl(), THING, Y).unwrap();
    assert!(!holds && !st.consistency_undetermined);
}

/// `C ⊑ owl:Nothing` is `C`'s unsatisfiability: flagged like `sat C` when
/// positive and undetermined; tautologies (`C ⊑ owl:Thing`, `owl:Nothing ⊑ C`,
/// `sat owl:Nothing`) are never flagged.
#[test]
fn bottom_and_tautology_answers_are_flagged_only_when_they_can_be_vacuous() {
    let _g = lock();
    let _wedge = SetEnvGuard::set("RUSTDL_WEDGE_CONSISTENCY", "0");
    let c = ctl();
    let (holds, st) = is_subclass_of_with_stats(&c, N, NOTHING).unwrap();
    assert!(holds && st.consistency_undetermined, "N ⊑ ⊥");
    let (holds, st) = is_subclass_of_with_stats(&c, Y, NOTHING).unwrap();
    assert!(!holds && !st.consistency_undetermined, "Y ⋢ ⊥");
    let (holds, st) = is_subclass_of_with_stats(&c, Y, THING).unwrap();
    assert!(holds && !st.consistency_undetermined, "Y ⊑ ⊤");
    let (holds, st) = is_subclass_of_with_stats(&c, NOTHING, Y).unwrap();
    assert!(holds && !st.consistency_undetermined, "⊥ ⊑ Y");
    let (sat, st) = is_class_satisfiable_with_stats(&c, NOTHING).unwrap();
    assert!(!sat && !st.consistency_undetermined, "sat ⊥");
}

/// The light lazy route builds no wedge above `RUSTDL_QUERY_LAZY_LIGHT_MAX_AXIOMS`
/// lowered axioms: the positive answer is kept and flagged, even on a KB only
/// the wedge finds inconsistent (the honest cost of the skip), for the named
/// and the expression queries alike.
#[test]
fn an_oversized_light_lazy_check_keeps_the_answer_and_flags_it() {
    use horned_owl::model::{Build, ClassExpression};
    let _g = lock();
    {
        let _max = SetEnvGuard::set("RUSTDL_QUERY_LAZY_LIGHT_MAX_AXIOMS", "1");
        let (sat, st) = is_class_satisfiable_with_stats(&ctl(), N).unwrap();
        assert!(!sat && st.consistency_undetermined, "ctl: unsat N flagged");
        let (holds, st) = is_subclass_of_with_stats(&p2(), Y, W).unwrap();
        assert!(
            holds && st.consistency_undetermined,
            "P2: Y ⊑ W kept and flagged"
        );
        let build: Build<RcStr> = Build::new_rc();
        let v = owl_dl_reasoner::class_expression_satisfiable(
            &ctl(),
            &ClassExpression::Class(build.class(N)),
        )
        .unwrap();
        assert!(
            !v.holds() && v.consistency_undetermined(),
            "sat-expr N flagged"
        );
    }
    // Default: the wedge decides ctl consistent, P2 inconsistent.
    let (sat, st) = is_class_satisfiable_with_stats(&ctl(), N).unwrap();
    assert!(!sat && !st.consistency_undetermined);
    assert!(is_inconsistent(&is_subclass_of_with_stats(&p2(), Y, W)));
}

/// The guard hands its EL closure to the query: answers and how they were
/// reached are those of `RUSTDL_CLASS_QUERY_GUARD=0`, which saturates in the
/// query itself.
#[test]
fn reusing_the_guard_closure_changes_no_answer() {
    const EL: &str = r"Prefix(:=<http://ex/#>)
Ontology(<http://ex/el>
  Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C))
  Declaration(ObjectProperty(:r)) Declaration(NamedIndividual(:i))
  SubClassOf(:A ObjectSomeValuesFrom(:r :B)) SubClassOf(ObjectSomeValuesFrom(:r :B) :C)
  ClassAssertion(:A :i)
)";
    let _g = lock();
    let el = parse(EL);
    let e = |s: &str| format!("http://ex/#{s}");
    let run = || {
        let mut out = Vec::new();
        for (o, classes) in [
            (
                ctl(),
                vec![X.to_owned(), Y.to_owned(), W.to_owned(), N.to_owned()],
            ),
            (el.clone(), vec![e("A"), e("B"), e("C")]),
        ] {
            for a in &classes {
                let (sat, st) = is_class_satisfiable_with_stats(&o, a).unwrap();
                out.push((sat, st.answered_by_saturation, st.pure_el_mode));
                for b in &classes {
                    let (h, st) = is_subclass_of_with_stats(&o, a, b).unwrap();
                    out.push((h, st.answered_by_saturation, st.pure_el_mode));
                }
            }
        }
        out
    };
    let on = run();
    let off = {
        let _off = SetEnvGuard::set("RUSTDL_CLASS_QUERY_GUARD", "0");
        run()
    };
    assert_eq!(on, off);
    // An ontology with an `ABox` is never pure-EL; the closure answers anyway.
    assert!(
        on.iter().any(|&(v, by_saturation, _)| v && by_saturation),
        "some answers come from the (reused) closure"
    );
}

/// `prove` runs the guard before the step proof (#428, option C): on an
/// inconsistent KB the entailment is reported entailed with `kb_inconsistent`
/// and the inconsistency core (#456), whether or not the saturator has a step
/// proof for it. A consistent KB never sets the flag.
#[test]
fn prove_reports_an_inconsistent_kb_whatever_answers_it() {
    use owl_dl_reasoner::{
        ProveEntailmentResult, prove_entailment, prove_entailment_rcstr_reporting,
    };
    let _g = lock();
    let o = parse(ABOX_CLASH);
    let (r, flags) = prove_entailment_rcstr_reporting(&o, X, Z).unwrap();
    assert!(flags.kb_inconsistent);
    match r {
        ProveEntailmentResult::JustificationFallback(j) => assert_eq!(j.axioms.len(), 3),
        other => panic!("{other:?}"),
    }
    // With a told `X ⊑ Z` the saturator has a step proof; the flag still comes.
    let told = parse(&ABOX_CLASH.replace(
        "DisjointClasses(:A :B)",
        "DisjointClasses(:A :B) SubClassOf(:X :Z)",
    ));
    let (r, flags) = prove_entailment_rcstr_reporting(&told, X, Z).unwrap();
    assert!(flags.kb_inconsistent);
    assert_is_the_abox_clash_core(&r);
    {
        // The guard, not the lazy check after the step proof, reports it: with
        // the wedge off the lazy check is undetermined.
        let _wedge = SetEnvGuard::set("RUSTDL_WEDGE_CONSISTENCY", "0");
        let (r, flags) = prove_entailment_rcstr_reporting(&told, X, Z).unwrap();
        assert!(flags.kb_inconsistent, "wedge off: {r:?}");
    }
    assert!(matches!(
        prove_entailment(&o, X, Z).unwrap(),
        ProveEntailmentResult::JustificationFallback(_)
    ));
    // Consistent: a step proof, no flag.
    let (r, flags) = prove_entailment_rcstr_reporting(&ctl(), Y, W).unwrap();
    assert!(!flags.kb_inconsistent);
    assert!(
        matches!(r, ProveEntailmentResult::SaturatorProof(_)),
        "{r:?}"
    );
    // A wedge-only inconsistency is found by the lazy check after the check
    // held: still entailed and flagged.
    let (r, flags) = prove_entailment_rcstr_reporting(&p2(), Y, W).unwrap();
    assert!(flags.kb_inconsistent, "P2");
    // #456: the justification is P2's inconsistency core (`{b} ⊑ ≥2 r`, `r`
    // functional), not the told `Y ⊑ W` that entails the subsumption.
    match &r {
        ProveEntailmentResult::JustificationFallback(j) => {
            let text = format!("{:?}", j.axioms);
            assert_eq!(j.axioms.len(), 2, "{text}");
            assert!(text.contains("FunctionalObjectProperty"), "{text}");
            assert!(text.contains("ObjectMinCardinality"), "{text}");
        }
        other => panic!("{other:?}"),
    }
    assert!(!flags.incomplete, "P2: the core search finished");
}

/// The fallback is exactly `ABOX_CLASH`'s three clash axioms, with no `SubClassOf`.
fn assert_is_the_abox_clash_core(r: &owl_dl_reasoner::ProveEntailmentResult) {
    match r {
        owl_dl_reasoner::ProveEntailmentResult::JustificationFallback(j) => {
            let text = format!("{:?}", j.axioms);
            assert_eq!(j.axioms.len(), 3, "{text}");
            assert!(!text.contains("SubClassOf"), "{text}");
            assert!(text.contains("DisjointClasses"), "{text}");
        }
        other => panic!("{other:?}"),
    }
}

/// #456: on an inconsistent KB, `prove` answers with the inconsistency core, not
/// a minimal set entailing `sub ⊑ sup`. Here `X ⊑ Y ⊑ Z` is a two-axiom
/// justification of `X ⊑ Z` that never touches the clash: before #456 `prove`
/// returned it (the `QuickXplain` search over `SubClassOf(X, Z)` reached it first);
/// now it returns the three clash axioms. The `ABox` saturation (the guard) sees
/// the clash; the P2 arm of the test above covers the #369 lazy-check route.
#[test]
fn prove_on_an_inconsistent_kb_answers_with_the_inconsistency_core() {
    use owl_dl_reasoner::prove_entailment_rcstr_reporting;
    let _g = lock();
    let chain = parse(&ABOX_CLASH.replace(
        "DisjointClasses(:A :B)",
        "Declaration(Class(:Y)) SubClassOf(:X :Y) SubClassOf(:Y :Z) DisjointClasses(:A :B)",
    ));
    let (r, flags) = prove_entailment_rcstr_reporting(&chain, X, Z).unwrap();
    assert!(flags.kb_inconsistent);
    assert!(!flags.incomplete);
    assert_is_the_abox_clash_core(&r);
    // The same answer for a pair with no entailment of its own.
    let (r, flags) = prove_entailment_rcstr_reporting(&chain, Z, X).unwrap();
    assert!(flags.kb_inconsistent);
    assert_is_the_abox_clash_core(&r);
}

/// `RUSTDL_QUERY_LAZY_LIGHT_MAX_AXIOMS` defaults to 5,000,000 (measured: nothing
/// in the ORE pool lies between 4.77 M and 14.1 M lowered axioms) and overrides.
#[test]
fn the_light_lazy_check_size_limit_defaults_to_five_million() {
    let _g = lock();
    assert_eq!(owl_dl_reasoner::LAZY_LIGHT_MAX_AXIOMS, 5_000_000);
    assert_eq!(owl_dl_reasoner::lazy_light_max_axioms(), 5_000_000);
    let _o = SetEnvGuard::set("RUSTDL_QUERY_LAZY_LIGHT_MAX_AXIOMS", "0");
    assert_eq!(owl_dl_reasoner::lazy_light_max_axioms(), 0);
}
