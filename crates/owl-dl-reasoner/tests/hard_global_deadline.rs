//! `RUSTDL_HARD_GLOBAL_DEADLINE` (#162): with a spent global budget, prep is held
//! to the deadline instead of running unbounded.
//!
//! Its own file because every test here sets process-wide env vars; cargo runs a
//! file's tests concurrently, so all of them take `ENV`.

#![allow(clippy::unwrap_used)]
#![allow(unsafe_code)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::Classification;
use std::fmt::Write as _;
use std::io::Cursor;
use std::sync::Mutex;
use std::time::Duration;

static ENV: Mutex<()> = Mutex::new(());

const N: usize = 3000;

/// `A_i ⊑ ∃r.B_i` and `∃r.B_i ⊑ C_i`: pure EL, and every `A_i ⊑ C_i` must be
/// DERIVED (no told edge links two reported classes), so a saturation cut at
/// once leaves nothing to report. That is the case the #162 empty-closure retry
/// exists for, which is why it can tell hard mode apart.
///
/// The saturator checks the clock every 4,096 worklist pops, so the cut depends
/// on POP COUNT, not machine speed. Measured with the CLI at a 1 ms budget in
/// hard mode: 500 rungs finish inside the first window, 1,000 abort, 1,500+
/// return nothing. 3,000 leaves a wide margin.
fn ladder(extra: &str) -> SetOntology<RcStr> {
    let mut src = String::from(
        "Prefix(:=<http://ex#>)\nPrefix(owl:=<http://www.w3.org/2002/07/owl#>)\nOntology(\n",
    );
    for i in 0..N {
        writeln!(src, "SubClassOf(:A{i} ObjectSomeValuesFrom(:r :B{i}))").unwrap();
        writeln!(src, "SubClassOf(ObjectSomeValuesFrom(:r :B{i}) :C{i})").unwrap();
    }
    src.push_str(extra);
    src.push(')');
    read_ofn(&mut Cursor::new(src), ParserConfiguration::default())
        .expect("parse")
        .0
}

/// Classify `onto` at a spent budget, `RUSTDL_PREP_DEADLINE` pinned ON (hard mode
/// requires it) and `RUSTDL_HARD_GLOBAL_DEADLINE` set as asked.
fn run(onto: &SetOntology<RcStr>, hard: bool) -> Classification {
    run_with(onto, hard, false)
}

/// As [`run`], with `RUSTDL_CONVERT_DEADLINE` chosen: `false` lets conversion
/// finish, so the tests of the PREPARATION cut still reach it with a zero budget.
fn run_with(onto: &SetOntology<RcStr>, hard: bool, cut_conversion: bool) -> Classification {
    let _g = ENV
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    // SAFETY: every env access in this binary holds `ENV`.
    unsafe {
        std::env::set_var("RUSTDL_PREP_DEADLINE", "1");
        std::env::set_var("RUSTDL_HARD_GLOBAL_DEADLINE", if hard { "1" } else { "0" });
        std::env::set_var(
            "RUSTDL_CONVERT_DEADLINE",
            if cut_conversion { "1" } else { "0" },
        );
    }
    let c = owl_dl_reasoner::classify_with_budget(onto, None, Some(Duration::ZERO)).unwrap();
    // SAFETY: as above.
    unsafe {
        std::env::remove_var("RUSTDL_PREP_DEADLINE");
        std::env::remove_var("RUSTDL_HARD_GLOBAL_DEADLINE");
        std::env::remove_var("RUSTDL_CONVERT_DEADLINE");
    }
    c
}

fn rung(c: &Classification, i: usize) -> bool {
    c.is_subclass(&format!("http://ex#A{i}"), &format!("http://ex#C{i}"))
}

/// Default: a spent budget leaves prep unbounded, the answer is complete, and the
/// fallback is reported.
#[test]
fn default_runs_prep_unbounded_on_a_spent_budget() {
    let c = run(&ladder(""), false);
    let st = c.stats();
    assert!(!st.prep_timed_out, "default prep runs to completion");
    assert!(
        st.prep_unbounded_budget_spent,
        "the unbounded fallback must be reported"
    );
    assert!(rung(&c, 0) && rung(&c, N - 1), "the complete answer");
}

/// Hard: prep is held to the spent deadline, saturation stops at once, the empty
/// partial answer is NOT retried unbounded, and every signal says so.
#[test]
fn hard_holds_prep_to_a_spent_deadline() {
    let c = run(&ladder(""), true);
    let st = c.stats();
    assert!(
        !st.prep_unbounded_budget_spent,
        "hard mode never leaves prep unbounded"
    );
    assert!(
        !st.prep_empty_retry,
        "hard mode must not re-run saturation unbounded"
    );
    assert!(st.prep_timed_out, "a cut prep must be flagged");
    assert!(!st.conversion_timed_out, "conversion was let finish");
    assert!(st.timed_out_pairs > 0, "the cut must drive `incomplete`");
    assert!(!c.completeness_guaranteed());
    assert!(
        st.consistency_undetermined,
        "a cut returns before the inconsistency pre-check, so consistency is unchecked"
    );
}

/// The case that makes the last assertion matter: an inconsistent KB. Default
/// finds it; hard mode cannot, and must not present `consistent` as determined.
#[test]
fn hard_mode_reports_an_unexamined_consistency_verdict() {
    let onto = ladder("SubClassOf(owl:Thing :E)\nSubClassOf(:E owl:Nothing)\n");
    let st = run(&onto, false).stats();
    assert!(
        st.inconsistent,
        "default prep finds the ⊤ ⊑ ⊥ inconsistency"
    );
    let st = run(&onto, true).stats();
    assert!(!st.inconsistent, "the cut closure cannot see it");
    assert!(st.consistency_undetermined, "and must say so");
}

/// Hard mode holds CONVERSION to the deadline too (#162). With a zero budget the
/// main loop stops at its first poll, so nothing is converted: the answer is
/// empty, which is sound, and must be flagged incomplete rather than reported
/// as a complete classification of an empty ontology.
#[test]
fn hard_mode_cuts_conversion_and_flags_the_answer_incomplete() {
    let c = run_with(&ladder(""), true, true);
    let st = c.stats();
    assert!(st.conversion_timed_out, "a zero budget cuts conversion");
    assert!(st.prep_timed_out, "the cut must reach the prep signal");
    assert!(st.timed_out_pairs > 0, "and drive `incomplete`");
    assert!(!c.completeness_guaranteed());
    assert!(!rung(&c, 0), "nothing was converted, so nothing is derived");
}

/// Default mode never cuts conversion.
#[test]
fn default_mode_converts_everything() {
    let c = run_with(&ladder(""), false, true);
    assert!(!c.stats().conversion_timed_out);
    assert!(rung(&c, N - 1));
}
