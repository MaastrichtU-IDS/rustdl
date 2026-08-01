//! Canaries for the classify PHASE ATTRIBUTION — `ClassificationStats`'s wall
//! breakdown must be MEASURED, not derived by subtraction.
//!
//! The defect: `tier_walk_wall_ms` was computed as
//! `total − (label_cache + snapshot_build + snapshot_replay)`, so every
//! unmeasured phase — the EL saturation, `from_internal`, the unsat probes, both
//! sweeps, the entailment-matrix BFS — was silently charged to the tier walk. On
//! `ore_ont_1028` the banner reported `tier_walk = 7198 ms` for a tier walk that
//! actually took 80 ms, and an earlier taxonomy of the DNF corpus was FALSIFIED
//! because of it: the instrument pointed at the wrong phase and a whole bucket
//! classification was built on the reading.
//!
//! Two properties are pinned:
//!
//! 1. **The components sum to the wall**, measured from OUTSIDE the call. This is
//!    the guard against a future residual reappearing: if someone adds an
//!    expensive phase without a timer, its cost lands in the explicitly named
//!    `unattributed_wall_ms`, and the bound on that value below fails.
//! 2. **`tier_walk` is not a sink** — it must fit in the wall left over once the
//!    other measured phases are accounted for. That is the exact shape of the old
//!    bug.
//!
//! Both are asserted on the HYBRID path (`∀` forces it), because the residual
//! only ever existed there.

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::{ClassificationStats, classify};
use std::io::Cursor;

fn parse(src: &str) -> SetOntology<RcStr> {
    let mut reader = Cursor::new(src.to_owned());
    let (onto, _) = read_ofn(&mut reader, ParserConfiguration::default()).expect("parse ofn");
    onto
}

/// The eight top-level, mutually disjoint phase line items, in execution order.
///
/// `snapshot_cache_build_wall_ms` / `snapshot_replay_wall_ms` are deliberately
/// ABSENT: they are NESTED sub-timers accumulated inside `decide` calls made by
/// the label-cache and tier-walk phases, so including them would double-count.
/// That nesting is exactly what made the old residual wrong in the other
/// direction (it subtracted them a second time).
fn phase_components(s: &ClassificationStats) -> [(&'static str, u64); 8] {
    [
        ("saturate", s.saturate_wall_ms),
        ("precheck", s.precheck_wall_ms),
        ("prepare", s.prepare_wall_ms),
        ("label_cache_build", s.label_cache_build_wall_ms),
        ("unsat_probe", s.unsat_probe_wall_ms),
        ("tier_walk", s.tier_walk_wall_ms),
        ("sweeps", s.sweep_wall_ms),
        ("matrix", s.matrix_wall_ms),
    ]
}

/// An out-of-fragment (`∀`) ontology with enough classes and definitions that
/// real time is spent in SEVERAL phases — saturation, `from_internal`, the label
/// cache, the tier walk and the matrix all get work. A trivial 3-class fixture
/// would leave every timer at 0 and make the sum assertion vacuous.
fn hybrid_ontology(k: usize) -> String {
    use std::fmt::Write as _;
    let mut s = String::from("Prefix(:=<http://e.org/>)\nOntology(\n");
    s.push_str("Declaration(ObjectProperty(:p))\nDeclaration(ObjectProperty(:q))\n");
    for i in 0..k {
        let _ = writeln!(s, "Declaration(Class(:A{i}))");
        let _ = writeln!(s, "Declaration(Class(:D{i}))");
    }
    for i in 0..k.saturating_sub(1) {
        let _ = writeln!(s, "SubClassOf(:A{i} :A{})", i + 1);
    }
    for i in 0..k {
        // `∀` on every class keeps the ontology off every saturation fast path.
        let _ = writeln!(
            s,
            "SubClassOf(:A{i} ObjectAllValuesFrom(:p :A{}))",
            (i + 1) % k
        );
        let _ = writeln!(
            s,
            "SubClassOf(:A{i} ObjectSomeValuesFrom(:q :A{}))",
            (i + 2) % k
        );
        // Defined classes give the label cache and the defined-sup sweep work.
        let _ = writeln!(
            s,
            "EquivalentClasses(:D{i} ObjectIntersectionOf(:A{i} ObjectSomeValuesFrom(:q :A{})))",
            (i + 3) % k
        );
    }
    s.push_str(")\n");
    s
}

/// **THE CANARY.** The reported components must account for the wall, and in
/// particular must not OVER-count it.
///
/// The bound is deliberately ASYMMETRIC, and that asymmetry is the whole point:
///
/// * **Over-counting is tightly bounded** (`outer + max(20, outer/20)`). The
///   phases are disjoint, strictly serial intervals inside one call, so their sum
///   can only exceed the outer wall if some interval is counted twice — which is
///   exactly what a residual does. Host noise cannot produce it.
/// * **Under-counting is bounded loosely, via `unattributed_wall_ms`**, because
///   the outer measurement legitimately includes `convert_ontology` and the
///   `Classification` return that sit outside the instrumented region.
///
/// A LOOSE symmetric tolerance is not enough: verified by sabotage — restoring
/// the old residual `tier_walk = total − label_cache − snapshots` passed a
/// `max(50, outer/4)` version of this test, because the ~490 ms of double-counted
/// prep fitted inside a 381 ms slack. It fails the bound below.
#[test]
fn phase_components_sum_to_the_measured_wall() {
    let onto = parse(&hybrid_ontology(60));
    let outer = std::time::Instant::now();
    let h = classify(&onto).expect("classify");
    let outer_ms = u64::try_from(outer.elapsed().as_millis()).unwrap_or(u64::MAX);
    let s = &h.stats();

    let parts = phase_components(s);
    let sum_parts: u64 = parts.iter().map(|&(_, v)| v).sum();
    let total_reported = sum_parts + s.unattributed_wall_ms;

    // Non-vacuity: with a sub-100 ms run every bound below is swallowed by its
    // own floor and the test proves nothing.
    assert!(
        outer_ms >= 200,
        "fixture ran in {outer_ms} ms — too fast for millisecond attribution to          be checkable; enlarge `hybrid_ontology`"
    );

    let over_tol = std::cmp::max(20, outer_ms / 20);
    assert!(
        total_reported <= outer_ms + over_tol,
        "phases OVER-count the wall by {} ms (> {over_tol}): sum(phases)={sum_parts}          + unattributed={} = {total_reported} vs outer={outer_ms}. Disjoint serial          intervals cannot do this — something is counted twice (a residual?).          parts={parts:?}",
        total_reported.saturating_sub(outer_ms),
        s.unattributed_wall_ms
    );

    // The residual must stay SMALL. This is the half that catches "someone added
    // a phase and forgot its timer" — the failure the old `tier_walk` residual
    // hid by absorbing it silently.
    // Tight, and it has to be: with `max(50, outer/4)` a sabotage that DELETED the
    // `prepare` timer entirely passed, because 323 ms of prepare fitted inside the
    // 381 ms slack. The measured baseline for this fixture is ~2 ms of genuinely
    // unattributed work (class-IRI/index build, `analyze_fragment`, tier grouping,
    // `Classification` assembly), so `max(30, outer/20)` still leaves a ~38x
    // margin while catching a whole missing phase.
    let under_tol = std::cmp::max(30, outer_ms / 20);
    assert!(
        s.unattributed_wall_ms <= under_tol,
        "unattributed_wall_ms={} exceeds {under_tol} ms — a phase is missing a \
         timer (parts={parts:?}, outer={outer_ms})",
        s.unattributed_wall_ms
    );
}

/// `tier_walk_wall_ms` specifically must not be a sink. Under the residual scheme
/// it absorbed every unmeasured phase — `ore_ont_1028` reported 7198 ms for an
/// 80 ms walk — so pin it against what the OTHER measured phases already claim:
/// once they are accounted for, there is only so much wall left, and a residual
/// `tier_walk` blows past it.
#[test]
fn tier_walk_is_not_a_residual_sink() {
    let onto = parse(&hybrid_ontology(60));
    let outer = std::time::Instant::now();
    let h = classify(&onto).expect("classify");
    let outer_ms = u64::try_from(outer.elapsed().as_millis()).unwrap_or(u64::MAX);
    let s = &h.stats();
    assert!(!s.pure_el_mode, "fixture must take the HYBRID path");
    assert!(
        outer_ms >= 200,
        "fixture too fast ({outer_ms} ms) to be checkable"
    );

    let parts = phase_components(s);
    let others: u64 = parts
        .iter()
        .filter(|&&(name, _)| name != "tier_walk")
        .map(|&(_, v)| v)
        .sum();
    let tol = std::cmp::max(20, outer_ms / 20);
    assert!(
        others + s.tier_walk_wall_ms <= outer_ms + tol,
        "tier_walk={} does not fit in the wall left after the other phases \
         ({others} of {outer_ms} ms, tol {tol}) — that is the residual-sink shape \
         this fix removed; parts={parts:?}",
        s.tier_walk_wall_ms
    );
    // Sanity: several phases must report time, or the fixture is not exercising
    // enough of the pipeline for the bound above to mean anything.
    let nonzero = parts.iter().filter(|&&(_, v)| v > 0).count();
    assert!(
        nonzero >= 2,
        "only {nonzero} phase(s) reported any time; parts={parts:?}"
    );
}

/// The saturation fast path used to report an ALL-ZERO breakdown, so a pure-EL
/// run that spent seconds in saturation looked like it spent nothing anywhere
/// (`ore_ont_10125` in the review: `label_cache_build=0 … tier_walk=0`).
#[test]
fn pure_el_fast_path_reports_its_phases() {
    use std::fmt::Write as _;
    // Pure-EL, but big enough for saturation to take measurable time.
    let mut src = String::from("Prefix(:=<http://e.org/>)\nOntology(\n");
    src.push_str("Declaration(ObjectProperty(:r))\n");
    for i in 0..900 {
        let _ = writeln!(src, "Declaration(Class(:A{i}))");
    }
    for i in 0..899 {
        let _ = writeln!(src, "SubClassOf(:A{i} :A{})", i + 1);
    }
    for i in (0..900).step_by(7) {
        let _ = writeln!(
            src,
            "SubClassOf(:A{i} ObjectSomeValuesFrom(:r :A{}))",
            (i + 3) % 900
        );
    }
    src.push_str(")\n");
    let onto = parse(&src);
    let h = classify(&onto).expect("classify");
    let s = &h.stats();
    assert!(s.pure_el_mode, "fixture must take the pure-EL fast path");
    assert!(
        s.saturate_wall_ms > 0,
        "the fast path must report its saturation time, not an all-zero breakdown"
    );
}
