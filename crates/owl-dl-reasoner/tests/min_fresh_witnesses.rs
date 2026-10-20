//! The `≥n R.C` rule in the main tableau (#304). Each verdict is HermiT's.
//!
//! `apply_min` counted every existing `R`-neighbour carrying `C` as a witness
//! and marked them pairwise distinct, generating only the shortfall. Two
//! existing witnesses need not be distinct, so a later `≤m` that had to merge
//! them read a satisfiable class as unsat. Now existing witnesses count only
//! when `n` of them are already pairwise distinct; otherwise `n` fresh ones are
//! made (`RUSTDL_MIN_FRESH_WITNESSES`, default ON).
//!
//! The wedge is OFF throughout so the main tableau is the engine answering.

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
        "Prefix(:=<http://t/>)\n\
         Ontology(\n\
         Declaration(Class(:A)) Declaration(Class(:B1)) Declaration(Class(:C))\n\
         Declaration(Class(:D1)) Declaration(Class(:D2)) Declaration(Class(:X))\n\
         Declaration(ObjectProperty(:r)) Declaration(ObjectProperty(:p))\n\
         {body}\n)"
    );
    let mut reader = Cursor::new(src);
    read_ofn(&mut reader, ParserConfiguration::default())
        .expect("parse")
        .0
}

/// `sat A` with the wedge off and the fix on or off.
fn sat(body: &str, fresh: bool) -> bool {
    let _lock = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _wedge = EnvGuard::set("RUSTDL_HYPERTABLEAU", "0");
    let _fresh = EnvGuard::set("RUSTDL_MIN_FRESH_WITNESSES", if fresh { "1" } else { "0" });
    owl_dl_reasoner::is_class_satisfiable(&onto(body), "http://t/A").expect("sat")
}

/// #304. `≥2 r.D1` finds y1 and y3, both `D1`, and used to mark them distinct.
/// `≤2 r.C` (every successor is `C` through `B1`) then had nowhere to put y2,
/// whose `∃p.¬X` clashes with the `∀p.X` of the others. The model merges y1 and
/// y3 and gives y2 a `D1`. HermiT: satisfiable.
const ISSUE_304: &str = "SubClassOf(:A ObjectMaxCardinality(2 :r :C))\n\
     SubClassOf(:A ObjectSomeValuesFrom(:r ObjectIntersectionOf(:D1 :D2 ObjectAllValuesFrom(:p :X))))\n\
     SubClassOf(:A ObjectSomeValuesFrom(:r ObjectIntersectionOf(:C :D2 ObjectSomeValuesFrom(:p ObjectComplementOf(:X)))))\n\
     SubClassOf(:A ObjectSomeValuesFrom(:r ObjectIntersectionOf(:C :D1 ObjectAllValuesFrom(:p :X))))\n\
     SubClassOf(:A :B1)\n\
     SubClassOf(:B1 ObjectAllValuesFrom(:r :C))\n\
     SubClassOf(:B1 ObjectMinCardinality(2 :r :D1))";

#[test]
fn existing_witnesses_are_not_marked_distinct_to_meet_a_min() {
    assert!(sat(ISSUE_304, true), "HermiT: A is satisfiable");
}

#[test]
fn the_old_rule_reads_issue_304_as_unsat() {
    // Control: the case above depends on the fix, not on the fixture alone.
    assert!(!sat(ISSUE_304, false));
}

/// `≥3 r.C` with `≤2 r.C`: three distinct successors cannot fit in two.
/// HermiT: unsatisfiable. Fresh witnesses must still be marked distinct.
#[test]
fn more_than_the_max_is_still_unsat() {
    let body = "SubClassOf(:A ObjectMinCardinality(3 :r :C))\n\
                SubClassOf(:A ObjectMaxCardinality(2 :r :C))";
    assert!(!sat(body, true));
}

/// `≥2 r.C` with two existing `C`-witnesses that cannot merge, and `≤2 r.C`:
/// the fresh pair must fit alongside them under the cap. HermiT: satisfiable,
/// by merging each fresh witness into an existing one.
#[test]
fn fresh_witnesses_can_merge_into_existing_ones() {
    let body = "SubClassOf(:A ObjectSomeValuesFrom(:r ObjectIntersectionOf(:C ObjectAllValuesFrom(:p :X))))\n\
                SubClassOf(:A ObjectSomeValuesFrom(:r ObjectIntersectionOf(:C ObjectSomeValuesFrom(:p ObjectComplementOf(:X)))))\n\
                SubClassOf(:A ObjectMinCardinality(2 :r :C))\n\
                SubClassOf(:A ObjectMaxCardinality(2 :r :C))";
    assert!(sat(body, true));
}

/// `≥2 r.D1` with `≤1 r.C` and every successor a `C`: two distinct successors
/// under a cap of one. HermiT: unsatisfiable.
#[test]
fn a_min_above_a_max_through_a_forall_is_unsat() {
    let body = "SubClassOf(:A ObjectMaxCardinality(1 :r :C))\n\
                SubClassOf(:A ObjectAllValuesFrom(:r :C))\n\
                SubClassOf(:A ObjectMinCardinality(2 :r :D1))";
    assert!(!sat(body, true));
}

/// Two existing `C`-witnesses, `≥2 r.C` and `≤1 r.C`. The existing pair is not
/// known distinct, so it cannot meet `≥2` on its own: counting it would let
/// `≤1` merge it into one successor and answer sat. HermiT: unsatisfiable.
#[test]
fn witnesses_not_known_distinct_do_not_meet_a_min() {
    let body = "SubClassOf(:A ObjectSomeValuesFrom(:r :C))\n\
                SubClassOf(:A ObjectSomeValuesFrom(:r ObjectIntersectionOf(:C :D1)))\n\
                SubClassOf(:A ObjectMinCardinality(2 :r :C))\n\
                SubClassOf(:A ObjectMaxCardinality(1 :r :C))";
    assert!(!sat(body, true));
}
