//! Regression canary for the blocked-node ⊔-rule termination fix
//! (`hyper.rs::find_open_disjunction` now skips directly-blocked nodes).
//!
//! WHY a dedicated canary: the Konclude closure-diff gate passes with OR without
//! this fix — without it, ore-15672's 109 `epistemic-workflow-enactment` pairs
//! Stall to the per-pair deadline and DEFAULT to "not subsumed", which happens
//! to be the correct verdict, so FP=0/MISSED=0 still holds. The closure-diff is
//! therefore blind to the bug. What the bug actually breaks is *decidability*:
//! the wedge cannot find a model for a satisfiable class, driving the disjunctive
//! search to any depth cap (256→32768 all Stalled, ~115k all-clashing branches).
//!
//! This canary asserts the fix's true signature: ore-15672 classifies with EVERY
//! pair DECIDED — `timed_out_pairs == 0` — at a modest per-pair budget. Without
//! the fix this is ~109; with it, 0 (and the whole classify drops 138s → ~0.05s).
//!
//! Needs the gitignored ore-15672 fixture (`scripts/fetch-real-ontologies.sh`).
//! Run: `cargo test -p owl-dl-reasoner --test blocked_disjunction_termination -- --ignored --nocapture`

#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::owx::reader::read as read_owx;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use std::io::BufReader;
use std::path::Path;
use std::time::Duration;

#[test]
#[ignore = "needs ore-15672 fixture; canary for the blocked-node ⊔ termination fix"]
fn ore15672_classifies_with_no_stalled_pairs() {
    let path = Path::new("../../ontologies/external/ore-15672-shoin-classified.owx");
    if !path.exists() {
        eprintln!("SKIP: missing ore-15672 fixture ({})", path.display());
        return;
    }
    let file = std::fs::File::open(path).expect("open ore-15672");
    let mut reader = BufReader::new(file);
    let (onto, _): (SetOntology<RcStr>, _) =
        read_owx(&mut reader, ParserConfiguration::default()).expect("parse ore-15672");

    // Generous per-pair budget: with the fix every pair decides in ms, so this
    // is never approached; without the fix ~109 pairs burn the full second each.
    let c = owl_dl_reasoner::classify_with_timeout(&onto, Duration::from_secs(1))
        .expect("classify_with_timeout returns Ok");
    let st = c.stats();
    eprintln!(
        "ore-15672: timed_out_pairs={} no_verdict_pairs(undecided)={}",
        st.timed_out_pairs,
        c.undecided_pairs().len()
    );
    assert_eq!(
        st.timed_out_pairs, 0,
        "blocked-node ⊔ rule must let every subsumption pair DECIDE \
         (non-zero ⇒ the disjunctive search is stalling on blocked-node \
         disjunctions again — the find_open_disjunction is_blocked skip regressed)"
    );
}
