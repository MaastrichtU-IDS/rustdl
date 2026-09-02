//! `is_instance_of` / `instances_of` now report truncation (#73).
//!
//! Both discarded a flag that `instance_check_reporting` already computed one call
//! down — a single `.map(|(is_instance, _truncated)| is_instance)`. A caller doing
//! a point membership check got `Ok(false)` with no way to tell a refutation from a
//! timeout, so a slow query silently became a negative answer. The only workaround
//! was to run the whole of `realize` just to read `incomplete()`.
//!
//! ## What the flag claims
//!
//! `(true, true)` is possible and is still a SOUND positive: the membership was
//! proved even though something else was cut. Only a `false` answer is called into
//! question. That is why these return a pair rather than an `Option<bool>` —
//! collapsing them would throw away a verdict that is known good.

#![allow(clippy::unwrap_used)]
#![allow(clippy::doc_markdown)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use std::fmt::Write as _;
use std::io::Cursor;

fn onto(body: &str) -> SetOntology<RcStr> {
    let src = format!("Prefix(:=<http://t/>)\nOntology(<http://t/x>\n{body}\n)");
    let mut reader = Cursor::new(src);
    read_ofn(&mut reader, ParserConfiguration::default())
        .expect("parse")
        .0
}

/// Disjunctive memberships: `a : (Ai ⊔ Bi)` with both disjuncts implying `Ci`, so
/// `a : Ci` holds only by CASE ANALYSIS and a real tableau probe must run. Same
/// shape `realize_incomplete_signal.rs` uses, and for the same reason: a told
/// closure answers before any probe, so a chain would not exercise truncation.
fn disjunctive(k: usize) -> String {
    let mut s = String::from("Declaration(NamedIndividual(:a))\n");
    for i in 0..k {
        let _ = write!(
            s,
            "Declaration(Class(:A{i})) Declaration(Class(:B{i})) Declaration(Class(:C{i}))\n\
             ClassAssertion(ObjectUnionOf(:A{i} :B{i}) :a)\n\
             SubClassOf(:A{i} :C{i})\n\
             SubClassOf(:B{i} :C{i})\n"
        );
    }
    s
}

/// Guard for an env var that must not leak into sibling tests.
struct EnvGuard(Option<std::ffi::OsString>);
impl Drop for EnvGuard {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        // SAFETY: see `set`.
        unsafe {
            match self.0.take() {
                Some(v) => std::env::set_var("RUSTDL_REALIZE_PAIR_TIMEOUT_MS", v),
                None => std::env::remove_var("RUSTDL_REALIZE_PAIR_TIMEOUT_MS"),
            }
        }
    }
}
#[allow(unsafe_code)]
fn set_budget(ms: &str) -> EnvGuard {
    let g = EnvGuard(std::env::var_os("RUSTDL_REALIZE_PAIR_TIMEOUT_MS"));
    // SAFETY: this binary contains ONE test, so no other thread reads the
    // environment while it is set — the same reason realize_incomplete_signal.rs
    // merged its two tests.
    unsafe {
        std::env::set_var("RUSTDL_REALIZE_PAIR_TIMEOUT_MS", ms);
    }
    g
}

/// Control and experiment in ONE test, for the reason recorded in
/// `realize_incomplete_signal.rs`: they share a process-global variable, and
/// splitting them into two `#[test]`s produced a race that failed ~1 run in 3.
#[test]
fn point_queries_report_whether_a_probe_was_cut() {
    // ── control: at the default budget every probe concludes, so nothing is cut
    // and the flag must be clear on BOTH surfaces.
    let o = onto(&disjunctive(60));
    let (is_inst, cut) = owl_dl_reasoner::is_instance_of_reporting(&o, "http://t/C0", "http://t/a")
        .expect("is_instance_of_reporting at the default budget");
    assert!(is_inst, "a : C0 holds by case analysis");
    assert!(!cut, "nothing is cut at the default budget");

    let (members, cut) = owl_dl_reasoner::instances_of_reporting(&o, "http://t/C0")
        .expect("instances_of_reporting at the default budget");
    assert_eq!(members, vec!["http://t/a".to_string()]);
    assert!(!cut, "nothing is cut at the default budget");

    // The un-reporting forms must agree with the reporting ones, so the refactor
    // did not change existing behaviour.
    assert_eq!(
        owl_dl_reasoner::is_instance_of(&o, "http://t/C0", "http://t/a").unwrap(),
        is_inst
    );
    assert_eq!(
        owl_dl_reasoner::instances_of(&o, "http://t/C0").unwrap(),
        members
    );

    // ── experiment: squeeze the per-pair budget and the probe is cut, so the flag
    // must fire. k is a RELIABILITY parameter — the deadline is per pair, so a
    // larger fixture makes "no probe cut" vanishingly unlikely; see the same
    // reasoning in realize_incomplete_signal.rs.
    let dense = onto(&disjunctive(240));
    let _g = set_budget("1");
    let (_, cut) = owl_dl_reasoner::instances_of_reporting(&dense, "http://t/C0")
        .expect("instances_of_reporting under a 1 ms budget");
    assert!(
        cut,
        "a 1 ms per-pair budget must cut at least one probe; if this fires, either \
         the fixture became decidable without probing or the flag is being discarded \
         again — the exact defect #73 reported"
    );
}
