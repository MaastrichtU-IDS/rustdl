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

/// Fixture size for the control leg: small enough that every probe concludes
/// inside the 750 ms default.
const CONTROL_K: usize = 60;

/// Fixture size for the truncation leg: large enough that at least one probe is
/// cut at 1 ms with probability ~1 - 1e-6. See the comment at its use.
const TRUNCATE_K: usize = 240;

/// Control and experiment in ONE test, deliberately.
///
/// These were two `#[test]`s sharing `RUSTDL_REALIZE_PAIR_TIMEOUT_MS` through an
/// `ENV_MUTEX`. That mutex serialises the critical sections, but `set_var` /
/// `remove_var` mutate PROCESS-GLOBAL state that threads outside the section can
/// still observe, and cargo runs a binary's tests concurrently. The result was a
/// flake in `truncated_probes_are_reported`: it failed roughly a third of the
/// time.
///
/// Measured, which is what identified the condition — 10 runs each:
///
/// | configuration | result |
/// |---|---|
/// | truncation test alone | 10/10 pass |
/// | both tests, `--test-threads=1` | 10/10 pass |
/// | both tests, default concurrency | ~1 in 3 FAIL |
///
/// So it was never host speed in the ordinary sense: alone, a 1 ms budget cuts a
/// probe every time (the CLI does too, 8/8). It was the concurrency between these
/// two. Merging them removes the race BY CONSTRUCTION rather than by widening a
/// budget until the symptom hides, and no coverage is lost — both assertions
/// still run, now in a guaranteed order.
///
/// A flaky gate is worse than a missing one: this one manufactured a false causal
/// story, appearing to blame an unrelated change that merely ran alongside it.
#[test]
fn incomplete_flag_tracks_whether_probes_were_cut() {
    // ── control: at the DEFAULT budget every probe concludes, so nothing is cut
    // and all `k` memberships are still found. Without this the flag could be
    // hard-wired true, or the fixture could be silently unsolvable.
    // THE TWO LEGS NEED DIFFERENT FIXTURE SIZES, and that is not arbitrary: the
    // control needs k SMALL enough that nothing is cut at the 750 ms default,
    // while the experiment needs k LARGE enough that something is reliably cut at
    // 1 ms. Measured — at k=240 the CONTROL fails 30/30, because some probe there
    // exceeds even 750 ms.
    let onto = parse(&disjunctive_memberships(CONTROL_K));
    let r = realize(&onto).expect("realize at the default budget");
    assert!(
        !r.incomplete(),
        "every probe concludes at the default 750 ms budget, so nothing was cut"
    );
    let types = r.entailed_types("http://t/a").len();
    assert!(
        types >= CONTROL_K,
        "the case-analysis memberships must be found at the default budget (got {types}); \
         if not, this fixture is not exercising what the test claims"
    );

    // ── experiment: squeeze the per-pair budget to 1 ms and the probes are cut,
    // so the flag must fire. This is precisely the state a consumer could not
    // detect before this field existed: the type set silently shrinks and nothing
    // says so.
    //
    // SAFETY: this binary now contains ONE test, so no other thread can observe
    // the process-global variable while it is set. That is the whole reason the
    // two tests were merged.
    #[allow(unsafe_code)]
    unsafe {
        std::env::set_var("RUSTDL_REALIZE_PAIR_TIMEOUT_MS", "1");
    }
    // k is a RELIABILITY PARAMETER here. The deadline is PER PAIR, so each of the
    // k probes independently either beats 1 ms or is cut, and the test fails only
    // when NONE is cut. Measured at k=60 that happened ~1 run in 30, implying
    // ~5.6% per probe, so P(none cut) = 0.944^k and k=240 puts it near 1e-6.
    // Raising k is the honest fix; widening the budget would hide the symptom
    // without making truncation deterministic.
    let dense = parse(&disjunctive_memberships(TRUNCATE_K));
    let cut = realize(&dense);
    #[allow(unsafe_code)]
    unsafe {
        std::env::remove_var("RUSTDL_REALIZE_PAIR_TIMEOUT_MS");
    }
    let cut = cut.expect("realize under a 1 ms per-pair budget");
    assert!(
        cut.incomplete(),
        "a 1 ms per-pair budget must cut at least one case-analysis probe; if this fires, \
         either the fixture became decidable without probing or the truncation is being \
         discarded again — the exact defect this field was added for"
    );
}
