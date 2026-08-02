//! Canaries for the **classify-path budget** on the `ABox`-saturation
//! inconsistency pre-check (`RUSTDL_CLASSIFY_INCONSISTENCY_MS`, default 3000).
//!
//! Making `RUSTDL_CLASSIFY_INCONSISTENCY` default-ON in v0.4.8 put an
//! *unbounded* named-individual fixpoint in front of every classify. On four
//! ORE ontologies with 60k–110k `ABox` assertions
//! (`ore_ont_{10838,15846,16315,3087}`) that fixpoint ate the whole run:
//! 1.3–4.4 s → **DNF at 60 s**. The budget abandons it on a deadline.
//!
//! Negatives first — the failure modes that would matter, in order:
//!
//! 1. **A timeout must never manufacture an inconsistency.** Abandoning is
//!    only sound because "no clash" was *already* the pre-check's no-verdict
//!    answer. If a timeout ever reported `clash`, every big-ABox classify
//!    would go wrong. (`timeout_never_reports_clash`,
//!    `timeout_never_makes_classify_inconsistent`)
//! 2. **A timeout must not leak a partial closure.** `edges` / `derived_same`
//!    feed `materialize_*` and `same_individuals`; a half-saturated set must
//!    be dropped, not published. (`timeout_publishes_no_partial_closure`)
//! 3. **The budget must not cut a fixpoint that fits.** A budget that fires
//!    early would silently drop real detections.
//!    (`generous_budget_matches_unbounded`, `default_budget_still_detects_*`)
//! 4. **The budget must not leak into the unbounded callers.** `is_consistent`
//!    is the surface for which this pre-check *is* the point of the call; a
//!    budget there would re-open the `family.ofn` correctness bug.
//!    (`is_consistent_ignores_the_classify_budget`)
//! 5. **The cut must actually happen inside the hot loops**, not only at the
//!    top of the fixpoint iteration — otherwise the first (unbounded) drain is
//!    exactly the cost we were trying to bound.
//!    (`tight_budget_cuts_in_{chain_rule,type_drain,edge_drain}`)
//!
//! Spec: `classify_inconsistency_budget_ms` +
//! `abox_saturation::saturate_abox_consistency_bounded` doc comments.

#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_core::ontology::InternalOntology;
use owl_dl_reasoner::abox_saturation::{
    SaturationResult, saturate_abox_consistency, saturate_abox_consistency_bounded,
};
use owl_dl_reasoner::{Classification, classify, is_consistent};
use std::fmt::Write as _;
use std::io::Cursor;
use std::time::{Duration, Instant};

// Env-mutation plumbing (mirrors `classify_inconsistency.rs`).
static ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct SetEnvGuard {
    key: &'static str,
    prior: Option<std::ffi::OsString>,
}

impl SetEnvGuard {
    #[allow(unsafe_code)]
    fn set(key: &'static str, value: &str) -> Self {
        let prior = std::env::var_os(key);
        // SAFETY: set_var is unsafe under edition 2024. Held for one test,
        // serialized via ENV_MUTEX, restored on Drop.
        unsafe {
            std::env::set_var(key, value);
        }
        Self { key, prior }
    }

    /// Remove the variable for the duration of the test (restored on Drop) —
    /// the only way to exercise a *shipped default* rather than a value the
    /// test itself chose.
    #[allow(unsafe_code)]
    fn unset(key: &'static str) -> Self {
        let prior = std::env::var_os(key);
        // SAFETY: see `set`.
        unsafe {
            std::env::remove_var(key);
        }
        Self { key, prior }
    }
}

impl Drop for SetEnvGuard {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        // SAFETY: see SetEnvGuard::set.
        unsafe {
            match &self.prior {
                Some(v) => std::env::set_var(self.key, v),
                None => std::env::remove_var(self.key),
            }
        }
    }
}

fn parse(src: &str) -> SetOntology<RcStr> {
    let mut reader = Cursor::new(src.to_owned());
    let (onto, _) = read_ofn(&mut reader, ParserConfiguration::default()).expect("parse ofn");
    onto
}

fn internal(src: &str) -> InternalOntology {
    owl_dl_core::convert::convert_ontology(&parse(src)).expect("convert")
}

/// `classify`'s view of consistency — what `classify --json` serialises into
/// its `"consistent"` field.
fn classify_says_consistent(c: &Classification) -> bool {
    !c.stats().inconsistent
}

// ── Fixtures ───────────────────────────────────────────────────────────────

/// Number of asserted hops in the "slow enough to be cut" fixture: `i0 →r i1
/// →r … →r i{CHAIN_N}`, i.e. `CHAIN_N + 1` individuals. With `r` transitive the
/// closure is `CHAIN_N(CHAIN_N + 1)/2` = 31 375 edges — far more than the 1 ms
/// budget these tests use can saturate even in a release build, while staying
/// a few seconds in the unoptimized test profile.
const CHAIN_N: usize = 250;
const CHAIN_EDGES: u64 = (CHAIN_N * (CHAIN_N + 1) / 2) as u64;

/// A CONSISTENT ontology whose `ABox` saturation is genuinely expensive: one
/// transitive role over a path of `CHAIN_N` named individuals.
fn transitive_chain(n: usize) -> String {
    let mut s = String::from(
        "Prefix(:=<http://example.org/chain#>)\n\
         Ontology(<http://example.org/chain>\n\
         Declaration(ObjectProperty(:r))\n\
         TransitiveObjectProperty(:r)\n",
    );
    for i in 0..n {
        let next = i + 1;
        let _ = write!(
            s,
            "Declaration(NamedIndividual(:i{i}))\nObjectPropertyAssertion(:r :i{i} :i{next})\n"
        );
    }
    let _ = write!(s, "Declaration(NamedIndividual(:i{n}))\n)\n");
    s
}

/// A SMALL genuinely `ABox`-inconsistent ontology, family-shaped: a functional
/// role whose two derived fillers carry told-disjoint types. Cheap enough that
/// the DEFAULT budget detects it in any build profile — which is the property
/// gate 2 cares about, without depending on the gitignored corpus or on host
/// speed.
const SMALL_INCONSISTENT: &str = r"Prefix(:=<http://example.org/tiny#>)
Ontology(<http://example.org/tiny>
  Declaration(Class(:Male))
  Declaration(Class(:Female))
  Declaration(ObjectProperty(:hasSex))
  Declaration(NamedIndividual(:a))
  Declaration(NamedIndividual(:m))
  Declaration(NamedIndividual(:f))
  DisjointClasses(:Male :Female)
  FunctionalObjectProperty(:hasSex)
  ClassAssertion(:Male :m)
  ClassAssertion(:Female :f)
  ObjectPropertyAssertion(:hasSex :a :m)
  ObjectPropertyAssertion(:hasSex :a :f)
)
";

/// SLOW **and** inconsistent: the `type_ladder` fan-out (so the clash is only
/// reachable after a ~`DRAIN_WORK`-addition worklist drain) plus a disjointness
/// that fires at the top of the ladder. Rule 8 runs at the END of a fixpoint
/// iteration, so any bound that cuts the drain misses this clash entirely —
/// which is exactly what makes it a discriminating fixture for "did the budget
/// leak into an unbounded caller?".
fn slow_inconsistent() -> String {
    let mut s = type_ladder();
    let top = LADDER;
    s.truncate(s.trim_end().len() - 1); // drop the closing ')'
    let _ = write!(
        s,
        "Declaration(Class(:d))\nDisjointClasses(:c{top} :d)\nClassAssertion(:d :x0)\n)\n"
    );
    s
}

// ── 1. A timeout must never manufacture an inconsistency ───────────────────

/// The soundness direction. An already-expired deadline abandons at the very
/// first probe; the verdict must be the no-verdict one (`clash == false`),
/// never a clash.
#[test]
fn timeout_never_reports_clash() {
    let onto = internal(SMALL_INCONSISTENT);

    // Control: unbounded, this fixture IS detected.
    let full = saturate_abox_consistency(&onto);
    assert!(full.clash, "precondition: the fixture clashes unbounded");
    assert!(!full.timed_out, "unbounded runs can never time out");

    // Expired deadline ⇒ no verdict, and specifically not a clash.
    let expired = saturate_abox_consistency_bounded(&onto, Some(Instant::now()));
    assert!(expired.timed_out, "an expired deadline must abandon");
    assert!(
        !expired.clash,
        "a timeout must NEVER report a clash — the whole safety argument is \
         that abandoning yields the pre-check's existing no-verdict answer"
    );
}

/// The same property one level up, at the surface users see: a KB that is
/// merely *expensive* must not be called inconsistent because the pre-check
/// ran out of budget.
#[test]
fn timeout_never_makes_classify_inconsistent() {
    let _lock = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let src = transitive_chain(CHAIN_N);
    let onto = parse(&src);

    let _flag = SetEnvGuard::set("RUSTDL_CLASSIFY_INCONSISTENCY", "1");
    let _budget = SetEnvGuard::set("RUSTDL_CLASSIFY_INCONSISTENCY_MS", "1");
    let classification = classify(&onto).expect("classify succeeds");
    assert!(
        classify_says_consistent(&classification),
        "the transitive chain is CONSISTENT; a budget expiry must not flip the \
         verdict"
    );
}

// ── 2. A timeout must not leak a partial closure ───────────────────────────

/// `edges` / `derived_same` feed `materialize_object_property_assertions` and
/// `same_individuals`. A partially saturated `edges` is still sound *per edge*
/// but is NOT the documented fixpoint set, so the abandoned result must
/// publish neither.
#[test]
fn timeout_publishes_no_partial_closure() {
    let onto = internal(&transitive_chain(CHAIN_N));

    let cut =
        saturate_abox_consistency_bounded(&onto, Some(Instant::now() + Duration::from_millis(1)));
    assert!(cut.timed_out, "1 ms cannot saturate {CHAIN_EDGES} edges");
    assert!(
        cut.edges.is_empty(),
        "abandoned run published {} edges — a partial closure must not be \
         mistaken for the fixpoint one",
        cut.edges.len()
    );
    assert!(
        cut.derived_same.is_empty(),
        "abandoned run published {} derived_same pairs",
        cut.derived_same.len()
    );
}

// ── 3. The budget must not cut a fixpoint that fits ────────────────────────

/// A budget far larger than the work must be indistinguishable from unbounded:
/// same verdict, same closure, and `timed_out` clear.
#[test]
fn generous_budget_matches_unbounded() {
    let onto = internal(&transitive_chain(CHAIN_N));

    let unbounded: SaturationResult = saturate_abox_consistency(&onto);
    let generous =
        saturate_abox_consistency_bounded(&onto, Some(Instant::now() + Duration::from_secs(600)));

    assert!(!generous.timed_out, "600 s is not a real bound here");
    assert_eq!(generous.clash, unbounded.clash);
    assert_eq!(
        generous.edges.len(),
        unbounded.edges.len(),
        "a generous budget must not truncate the closure"
    );
    assert_eq!(
        unbounded.edges.len() as u64,
        CHAIN_EDGES,
        "sanity: the transitive closure of a {CHAIN_N}-hop path is n(n+1)/2 edges"
    );
}

/// Gate 2 in miniature, and profile-independent: with the DEFAULT budget (no
/// env override at all) `classify` must still see a real `ABox` inconsistency
/// and must still agree with `is_consistent`.
///
/// Deliberately NOT `family.ofn`: that fixture needs ~2.0 s of saturation in a
/// release build and several times that in the unoptimized test profile, so a
/// canary built on it would be measuring the host, not the code. The
/// release-build `family.ofn` check is a CLI-level gate instead.
#[test]
fn default_budget_still_detects_small_abox_inconsistency() {
    let _lock = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let onto = parse(SMALL_INCONSISTENT);

    assert!(
        !is_consistent(&onto).expect("is_consistent succeeds"),
        "precondition: functional role with told-disjoint fillers is inconsistent"
    );

    let _flag = SetEnvGuard::set("RUSTDL_CLASSIFY_INCONSISTENCY", "1");
    // Unset, so this exercises the SHIPPED default, not a value chosen here.
    let _clear = SetEnvGuard::unset("RUSTDL_CLASSIFY_INCONSISTENCY_MS");
    let classification = classify(&onto).expect("classify succeeds");
    assert!(
        !classify_says_consistent(&classification),
        "classify must still detect a cheap ABox inconsistency under the \
         default budget — if this fires, the budget is wrong, not the KB"
    );
}

/// The same, on a fixture that is **not** cheap: the clash sits behind a
/// ~`DRAIN_WORK`-addition drain.
///
/// **HONEST LIMITATION, established by sabotage rather than assumed: neither
/// this test nor the one above pins the 3000 ms NUMBER.** Slashing the default
/// to 1 ms leaves both green. The cheap fixture saturates in microseconds; and
/// this one turns out to be detected by classify through a route that is not
/// the budgeted pre-check at all (it still reports `consistent: false` under
/// `RUSTDL_CLASSIFY_INCONSISTENCY=0` *and* under `RUSTDL_ABOX_CHECK=0`).
///
/// That is not a fixable oversight in the fixture: the pre-check is
/// load-bearing precisely where every cheaper route fails, and the known
/// example of that is `family.ofn` itself — corpus-gated, and needing ~2.0 s
/// of saturation in release (far more in this profile), so it cannot pin a
/// millisecond default from inside the test suite. **The default is validated
/// by the release-CLI `family.ofn` measurement recorded in `CHANGELOG.md`;
/// re-measure `family` before changing it.** What these two tests do pin is
/// that the signal is wired and that the budget does not break it.
#[test]
fn default_budget_still_detects_slow_abox_inconsistency() {
    let _lock = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let src = slow_inconsistent();
    let onto = parse(&src);

    assert!(
        saturate_abox_consistency(&internal(&src)).clash,
        "precondition: the slow fixture clashes at fixpoint"
    );

    let _flag = SetEnvGuard::set("RUSTDL_CLASSIFY_INCONSISTENCY", "1");
    let _clear = SetEnvGuard::unset("RUSTDL_CLASSIFY_INCONSISTENCY_MS");
    let classification = classify(&onto).expect("classify succeeds");
    assert!(
        !classify_says_consistent(&classification),
        "the shipped default budget is too small to reach a clash that needs a \
         full worklist drain"
    );
}

// ── 4. The budget must not leak into the unbounded callers ─────────────────

/// `is_consistent` is the caller for which the pre-check *is* the answer
/// (`family.ofn`: `HermiT`, Konclude and rustdl all call it inconsistent). Even
/// with the classify budget pinned to 1 ms it must be unaffected — the budget
/// is threaded only through `classify_inconsistency_precheck`.
///
/// The fixture has to be SLOW as well as inconsistent, which is the point of
/// [`slow_inconsistent`]: sabotage showed that with a *cheap* inconsistent
/// fixture this test passes even when the budget is deliberately wired into
/// the unbounded entry point, because a microsecond fixpoint never notices a
/// 1 ms bound. Here the clash is only reachable after a ~90k-addition drain.
#[test]
fn is_consistent_ignores_the_classify_budget() {
    let _lock = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let src = slow_inconsistent();
    let onto = parse(&src);

    // Precondition, budget-free: the fixture really is detected by the
    // ABox-saturation pre-check.
    assert!(
        saturate_abox_consistency(&internal(&src)).clash,
        "precondition: the slow fixture clashes at fixpoint"
    );

    let _budget = SetEnvGuard::set("RUSTDL_CLASSIFY_INCONSISTENCY_MS", "1");
    assert!(
        !is_consistent(&onto).expect("is_consistent succeeds"),
        "the classify budget leaked into is_consistent — that re-opens the \
         family.ofn correctness bug the pre-check was added for"
    );
}

/// The other unbounded consumer, and the one whose *output* (not just a
/// verdict) comes straight from the fixpoint:
/// `materialize_object_property_assertions` publishes the derived edge set.
/// With the classify budget pinned to 1 ms it must still return the FULL
/// transitive closure — i.e. no env-driven bound may reach
/// `saturate_abox_consistency` itself.
///
/// This is the leak test with teeth. `is_consistent` has a complete tableau
/// fallback, so a leak there is invisible on any fixture the tableau can also
/// solve; a truncated closure here is directly observable.
#[test]
fn materialize_ignores_the_classify_budget() {
    let _lock = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let onto = parse(&transitive_chain(CHAIN_N));
    let _budget = SetEnvGuard::set("RUSTDL_CLASSIFY_INCONSISTENCY_MS", "1");

    let edges = owl_dl_reasoner::materialize_object_property_assertions(&onto)
        .expect("materialize succeeds");
    assert_eq!(
        edges.len() as u64,
        CHAIN_EDGES,
        "materialize published a TRUNCATED closure — an env-driven budget \
         reached the unbounded saturator entry point"
    );
}

// ── 5. The cut must happen inside the hot loops ────────────────────────────

/// The **role-chain** rules must be probed. The transitive closure of a path
/// is built one hop-length per outer iteration, so nearly all of this
/// fixture's work happens in the `chains2` loop, not in a worklist drain.
///
/// The bound is deliberately loose (half the closure) so it measures
/// *structure*, not host speed.
///
/// NOTE, verified by sabotage rather than assumed: deleting the two
/// *worklist-drain* probes leaves this test GREEN (each drain here is short —
/// the closure grows through the chain rule), which is why the two tests below
/// exist. One "the deadline is probed somewhere inside" assertion is not the
/// same as guarding each site.
#[test]
fn tight_budget_cuts_in_chain_rule() {
    let onto = internal(&transitive_chain(CHAIN_N));

    let cut =
        saturate_abox_consistency_bounded(&onto, Some(Instant::now() + Duration::from_millis(1)));
    assert!(cut.timed_out);
    assert!(
        cut.edge_additions < CHAIN_EDGES / 2,
        "abandoned after {} of {CHAIN_EDGES} edge additions — the deadline is \
         not being probed inside the role-chain loops",
        cut.edge_additions
    );
}

/// Fan-out width for the single-drain fixtures below.
const FAN: usize = 300;
/// Depth of the `SubClassOf` / `SubObjectPropertyOf` ladder each seed climbs.
const LADDER: usize = 300;
/// Work the FIRST worklist drain does on those fixtures, in one uninterrupted
/// pass: every one of `FAN` seeds walks the whole `LADDER`.
const DRAIN_WORK: u64 = (FAN * LADDER) as u64;

/// `FAN` individuals typed `:c0`, plus a ladder `c0 ⊑ c1 ⊑ … ⊑ c{LADDER}`.
/// Every derived type lands in the SAME `type_queue` drain, so the whole
/// `DRAIN_WORK` happens before the fixpoint loop can come back round to its
/// per-iteration probe. Consistent — no disjointness anywhere.
fn type_ladder() -> String {
    let mut s = String::from(
        "Prefix(:=<http://example.org/ladder#>)\nOntology(<http://example.org/ladder>\n",
    );
    for k in 0..LADDER {
        let next = k + 1;
        let _ = writeln!(s, "SubClassOf(:c{k} :c{next})");
    }
    for i in 0..FAN {
        let _ = write!(
            s,
            "Declaration(NamedIndividual(:x{i}))\nClassAssertion(:c0 :x{i})\n"
        );
    }
    s.push_str(")\n");
    s
}

/// The `edge_queue` analogue: `FAN` asserted `r0` edges and a role ladder
/// `r0 ⊑ r1 ⊑ … ⊑ r{LADDER}`, so each asserted edge is re-derived up the whole
/// hierarchy inside the first drain.
fn role_ladder() -> String {
    let mut s = String::from(
        "Prefix(:=<http://example.org/rladder#>)\nOntology(<http://example.org/rladder>\n",
    );
    for k in 0..=LADDER {
        let _ = writeln!(s, "Declaration(ObjectProperty(:r{k}))");
    }
    for k in 0..LADDER {
        let next = k + 1;
        let _ = writeln!(s, "SubObjectPropertyOf(:r{k} :r{next})");
    }
    for i in 0..FAN {
        let _ = write!(
            s,
            "Declaration(NamedIndividual(:a{i}))\nDeclaration(NamedIndividual(:b{i}))\n\
             ObjectPropertyAssertion(:r0 :a{i} :b{i})\n"
        );
    }
    s.push_str(")\n");
    s
}

/// Guards the probe in the **type-queue** drain specifically. Without it the
/// first drain runs `DRAIN_WORK` type additions uninterrupted, however small
/// the budget.
#[test]
fn tight_budget_cuts_in_type_drain() {
    let onto = internal(&type_ladder());

    let full = saturate_abox_consistency(&onto);
    assert!(
        full.type_additions >= DRAIN_WORK,
        "sanity: {FAN} seeds × a {LADDER}-step ladder, got {}",
        full.type_additions
    );

    let cut =
        saturate_abox_consistency_bounded(&onto, Some(Instant::now() + Duration::from_millis(1)));
    assert!(cut.timed_out, "1 ms cannot do {DRAIN_WORK} type additions");
    assert!(
        cut.type_additions < DRAIN_WORK / 2,
        "abandoned after {} of {DRAIN_WORK} type additions — the deadline is \
         not being probed inside the TYPE-queue drain",
        cut.type_additions
    );
}

/// Guards the probe in the **edge-queue** drain specifically.
#[test]
fn tight_budget_cuts_in_edge_drain() {
    let onto = internal(&role_ladder());

    let full = saturate_abox_consistency(&onto);
    assert!(
        full.edge_additions >= DRAIN_WORK,
        "sanity: {FAN} edges × a {LADDER}-step role ladder, got {}",
        full.edge_additions
    );

    let cut =
        saturate_abox_consistency_bounded(&onto, Some(Instant::now() + Duration::from_millis(1)));
    assert!(cut.timed_out, "1 ms cannot do {DRAIN_WORK} edge additions");
    assert!(
        cut.edge_additions < DRAIN_WORK / 2,
        "abandoned after {} of {DRAIN_WORK} edge additions — the deadline is \
         not being probed inside the EDGE-queue drain",
        cut.edge_additions
    );
}

/// Control for the test above: `None` must not read the clock at all, and must
/// reach the full fixpoint.
#[test]
fn unbounded_is_never_cut() {
    let onto = internal(&transitive_chain(CHAIN_N));
    let full = saturate_abox_consistency_bounded(&onto, None);
    assert!(!full.timed_out);
    assert_eq!(full.edges.len() as u64, CHAIN_EDGES);
}
