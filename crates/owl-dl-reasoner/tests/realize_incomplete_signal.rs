//! `realize` now reports whether an instance probe was CUT rather than refuted.
//!
//! # Why this exists
//!
//! A deadline expiry, a `RUSTDL_MAX_NODES` trip and a depth-limit bail all collapse
//! to `Ok(false)` = "not an instance" in `instance_check_with_closure`. That is a sound
//! under-approximation and the code always said so — but the information was
//! discarded at the source, so `realize --json` shipped with **no completeness signal
//! at all** while `classify --json` has had one for months. A consumer could not
//! distinguish "these are all the types" from "some were dropped", and the per-pair
//! budget defaults to 750 ms so truncation is reachable on real inputs.
//!
//! Same shape as `classify::prep_bounding_decision`, added the same day for the same
//! reason: a silent sound-under-approximation the caller cannot observe.
//!
//! # Direction of the guarantee
//!
//! `incomplete == true` means something was cut. `false` means nothing was cut **on
//! the path taken** — it does NOT certify that no entailment is missed by another
//! mechanism, e.g. the derived-equality gap in
//! `docs/known-limitations/realize-drops-derived-individual-equality.md`.

use owl_dl_reasoner::realize;
use std::fmt::Write as _;
use std::sync::Mutex;

/// Serialises the two tests below. They share `RUSTDL_REALIZE_PAIR_TIMEOUT_MS`, and
/// cargo runs tests in one binary CONCURRENTLY — without this the truncation test sets
/// the var to 1 ms while the control is mid-realize, and the control fails with
/// "nothing was cut" pointing at the wrong thing entirely. Cost one debugging cycle.
static ENV_MUTEX: Mutex<()> = Mutex::new(());

fn parse(src: &str) -> horned_owl::ontology::set::SetOntology<horned_owl::model::RcStr> {
    horned_owl::io::ofn::reader::read(
        &mut std::io::Cursor::new(src.to_string()),
        horned_owl::io::ParserConfiguration::default(),
    )
    .expect("parse")
    .0
}

/// Disjunctive class assertions: `a : (Ai ⊔ Bi)` with both disjuncts implying `Ci`,
/// so `a : Ci` holds but only by CASE ANALYSIS — the told-closure cannot answer it and
/// a real tableau probe must run.
///
/// This shape was arrived at empirically. A plain subsumption chain does NOT work:
/// `instance_check_reporting` answers from `closure.contains(told, class_id)` before any
/// probe, so a 1 ms budget over 120 chained classes still returned all 120 types with
/// `incomplete == false` — correctly, because nothing was cut. A canary for a probe
/// signal has to force a probe.
fn disjunctive_memberships(k: usize) -> String {
    let mut s = String::from("Prefix(:=<http://t/>)\nOntology(<http://t/inc>\n");
    s.push_str("Declaration(NamedIndividual(:a))\n");
    for i in 0..k {
        let _ = write!(
            s,
            "Declaration(Class(:A{i})) Declaration(Class(:B{i})) Declaration(Class(:C{i}))\n\
             ClassAssertion(ObjectUnionOf(:A{i} :B{i}) :a)\n\
             SubClassOf(:A{i} :C{i})\n\
             SubClassOf(:B{i} :C{i})\n"
        );
    }
    s.push(')');
    s
}

/// At the DEFAULT per-pair budget every probe concludes, so nothing is cut and the flag
/// must be clear — and all `k` memberships must still be found. Without this control the
/// flag could be hard-wired true, or the fixture could be silently unsolvable.
#[test]
fn a_decidable_realization_is_not_flagged_incomplete() {
    let _lock = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let onto = parse(&disjunctive_memberships(60));
    let r = realize(&onto).expect("realize at the default budget");
    assert!(
        !r.incomplete(),
        "every probe concludes at the default 750 ms budget, so nothing was cut"
    );
    let types = r.entailed_types("http://t/a").len();
    assert!(
        types >= 60,
        "the case-analysis memberships must be found at the default budget (got {types});          if not, this fixture is not exercising what the test claims"
    );
}

/// With the per-pair budget squeezed to 1 ms the probes are cut, and the flag must fire.
/// This is precisely the state a consumer could not detect before this field existed:
/// the type set silently shrinks and nothing says so.
#[test]
fn truncated_probes_are_reported() {
    let _lock = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    // SAFETY: this test owns the variable; no other test in this file reads it.
    #[allow(unsafe_code)]
    unsafe {
        std::env::set_var("RUSTDL_REALIZE_PAIR_TIMEOUT_MS", "1");
    }
    let onto = parse(&disjunctive_memberships(60));
    let r = realize(&onto);
    #[allow(unsafe_code)]
    unsafe {
        std::env::remove_var("RUSTDL_REALIZE_PAIR_TIMEOUT_MS");
    }
    let r = r.expect("realize under a 1 ms per-pair budget");
    assert!(
        r.incomplete(),
        "a 1 ms per-pair budget must cut at least one case-analysis probe; if this fires,          either the fixture became decidable without probing or the truncation is being          discarded again — the exact defect this field was added for"
    );
}
