//! Which pre-check routes report `consistency_undetermined` when the global
//! deadline cuts them (#162, review of #227).
//!
//! Its own file because every test here sets process-wide env vars; cargo runs a
//! file's tests concurrently, so all of them take `ENV`.

#![allow(clippy::unwrap_used)]
#![allow(unsafe_code)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use std::io::Cursor;
use std::sync::Mutex;
use std::time::Duration;

static ENV: Mutex<()> = Mutex::new(());

fn onto() -> SetOntology<RcStr> {
    // An ABox-bearing KB, consistent, so nothing but the flag can change.
    let src = "Prefix(:=<http://ex#>)\nOntology(\n\
               Declaration(Class(:A)) Declaration(NamedIndividual(:a))\n\
               ClassAssertion(:A :a)\n)";
    read_ofn(&mut Cursor::new(src), ParserConfiguration::default())
        .expect("parse")
        .0
}

/// `consistency_undetermined` at a spent global budget, with `vars` set to `"0"`.
fn undetermined_at_zero_budget(vars: &[&str]) -> bool {
    let _g = ENV
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    for v in vars {
        // SAFETY: every env access in this binary holds `ENV`.
        unsafe { std::env::set_var(v, "0") };
    }
    let u = owl_dl_reasoner::classify_with_global_deadline(&onto(), Duration::ZERO)
        .unwrap()
        .stats()
        .consistency_undetermined;
    for v in vars {
        // SAFETY: as above.
        unsafe { std::env::remove_var(v) };
    }
    u
}

/// With the wedge route off, the `ABox`-saturation route alone is cut by the spent
/// global budget. Before the review fix that cut was silent.
#[test]
fn a_cut_abox_route_is_reported_when_no_wedge_runs() {
    assert!(
        undetermined_at_zero_budget(&["RUSTDL_CLASSIFY_WEDGE_INCONSISTENCY"]),
        "a cut ABox route with no wedge verdict must be reported"
    );
}

/// With both routes disabled nothing was skipped, so nothing is reported.
#[test]
fn disabled_routes_are_not_reported_as_skipped() {
    assert!(
        !undetermined_at_zero_budget(&[
            "RUSTDL_CLASSIFY_WEDGE_INCONSISTENCY",
            "RUSTDL_ABOX_SATURATION"
        ]),
        "no enabled route was skipped, so consistency is not undetermined"
    );
}
