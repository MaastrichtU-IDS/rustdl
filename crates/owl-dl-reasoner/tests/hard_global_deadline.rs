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
use std::fmt::Write as _;
use std::io::Cursor;
use std::sync::Mutex;
use std::time::Duration;

static ENV: Mutex<()> = Mutex::new(());

/// `A_i ⊑ ∃r.B_i` and `∃r.B_i ⊑ C_i`: pure EL, and every `A_i ⊑ C_i` must be
/// DERIVED (no told edge links two reported classes), so a saturation cut at
/// once leaves nothing to report. That is the case the #162 empty-closure retry
/// exists for, which is why it can tell hard mode apart. Large enough to outlast
/// the saturator's first deadline check (every 4,096 worklist pops).
fn ladder(n: usize) -> SetOntology<RcStr> {
    let mut src = String::from("Prefix(:=<http://ex#>)\nOntology(\n");
    for i in 0..n {
        writeln!(src, "SubClassOf(:A{i} ObjectSomeValuesFrom(:r :B{i}))").unwrap();
        writeln!(src, "SubClassOf(ObjectSomeValuesFrom(:r :B{i}) :C{i})").unwrap();
    }
    src.push(')');
    read_ofn(&mut Cursor::new(src), ParserConfiguration::default())
        .expect("parse")
        .0
}

/// `(prep_timed_out, prep_empty_retry, prep_unbounded_budget_spent)` at a spent
/// budget.
fn run(hard: bool) -> (bool, bool, bool) {
    let _g = ENV
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    // SAFETY: every env access in this binary holds `ENV`.
    unsafe { std::env::set_var("RUSTDL_HARD_GLOBAL_DEADLINE", if hard { "1" } else { "0" }) };
    let c =
        owl_dl_reasoner::classify_with_budget(&ladder(3000), None, Some(Duration::ZERO)).unwrap();
    // SAFETY: as above.
    unsafe { std::env::remove_var("RUSTDL_HARD_GLOBAL_DEADLINE") };
    let st = c.stats();
    (
        st.prep_timed_out,
        st.prep_empty_retry,
        st.prep_unbounded_budget_spent,
    )
}

/// Default: a spent budget leaves prep unbounded, the answer is complete, and the
/// fallback is reported.
#[test]
fn default_runs_prep_unbounded_on_a_spent_budget() {
    let (cut, _retry, unbounded) = run(false);
    assert!(!cut, "default prep runs to completion");
    assert!(unbounded, "the unbounded fallback must be reported");
}

/// Hard: prep is held to the spent deadline, saturation stops at once, the empty
/// partial answer is NOT retried unbounded, and it is flagged.
#[test]
fn hard_holds_prep_to_a_spent_deadline() {
    let (cut, retry, unbounded) = run(true);
    assert!(!unbounded, "hard mode never leaves prep unbounded");
    assert!(!retry, "hard mode must not re-run saturation unbounded");
    assert!(cut, "a cut prep must be flagged");
}
