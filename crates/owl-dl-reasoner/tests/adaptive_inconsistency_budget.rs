//! Canaries for the **adaptive** classify-path budget on the `ABox`-saturation
//! inconsistency pre-check
//! ([`adaptive_classify_inconsistency_budget_ms`](owl_dl_reasoner::adaptive_classify_inconsistency_budget_ms)).
//!
//! ## Why this file exists separately from `classify_inconsistency_budget.rs`
//!
//! That file's own doc comment records an honest limitation: **none of its 12
//! canaries pinned the 3000 ms VALUE** — slashing the default to 1 ms left them
//! all green, because its cheap fixture saturates in microseconds and its slow
//! fixture is detected by a route that is not the budgeted pre-check at all.
//! The only known ontology where this pre-check is genuinely load-bearing is
//! `family.ofn`, which is corpus-gated *and* costs **37.9 s in this
//! (unoptimized) test profile** against ~2.6 s in release — so it cannot pin a
//! millisecond default from inside a normal `cargo test` run.
//!
//! The fix is to pin the **rule** rather than the wall. The rule is a pure
//! function of two integers, so it can be tested exhaustively and cheaply at
//! the exact predictor values measured off the real ontologies. A mutation that
//! starves `family` therefore fails a test here, in any profile, in
//! milliseconds.
//!
//! ## Negatives first — the failure modes, in order
//!
//! 1. **The rule must not starve `family.ofn`.** It is the one ontology this
//!    pre-check exists for; the flat 3000 ms left it ~13% headroom, which is
//!    the fragility this change is fixing. (`family_predictors_get_generous_budget`,
//!    `family_budget_has_at_least_3x_headroom`)
//! 2. **…and must not overshoot into effectively unbounded either.** ADDED
//!    BECAUSE A SABOTAGE SURVIVED: blowing the generous budget to 1 000 000 ms
//!    left every other canary green, which is the v0.4.8 defect re-entering
//!    through the low-work door. (`generous_budget_is_bounded_above`)
//! 3. **The direction must not invert.** Scaling the budget *up* with `ABox`
//!    size is the intuitive rule and it is backwards: the expensive cases are
//!    expensive *because* the `ABox` closure runs away, so an increasing budget
//!    starves `family` (508 individuals) and subsidises the four ontologies
//!    whose DNF the budget exists to prevent.
//!    (`budget_is_monotone_non_increasing_in_work`)
//! 4. **The rule must never grant LESS than the superseded flat default.** That
//!    is what keeps `ore_ont_{10838,15846,16315,3087}` at the walls the flat
//!    3000 ms bought them: outside the low-work class the budget is bit-identical
//!    to today. (`never_below_the_superseded_flat_default`,
//!    `regression_shaped_predictors_get_exactly_the_old_default`)
//! 5. **A huge chain-free `ABox` must not fall into the low-work class.** The
//!    work proxy multiplies by `max(multiplying_rules, 1)` precisely so that
//!    `multiplying_rules == 0` does not collapse the score to zero.
//!    (`chain_free_but_enormous_abox_is_not_low_work`)
//! 6. **…but the PRELUDE-dominated ontologies must stay generous.** A second,
//!    independent cost driver exists (the fixpoint's per-axiom pre-indexing
//!    prelude, 18.6 M axioms on `ore_ont_5368`) and it is deliberately NOT
//!    modelled, because it is budget-INDEPENDENT: gating on axiom count would
//!    make those cases strictly worse. This pins the measurement against the
//!    reflex. (`prelude_dominated_predictors_stay_generous`)
//! 7. **The explicit override must still win, including `0` for unbounded.**
//!    (`env_override_wins`, `env_override_zero_means_unbounded`)
//! 8. **The predictor pass must count what it claims**, or every threshold above
//!    is calibrated against a lie. (`predictors_count_edges_and_multipliers`)
//!
//! Spec: `docs/2026-08-03-adaptive-inconsistency-budget.md`.

#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_core::ontology::InternalOntology;
use owl_dl_reasoner::{
    AboxCostPredictors, INCONSISTENCY_GENEROUS_MS, INCONSISTENCY_STINGY_MS,
    INCONSISTENCY_WORK_THRESHOLD, abox_cost_predictors, adaptive_classify_inconsistency_budget_ms,
    classify_inconsistency_budget_ms,
};
use std::io::Cursor;

// ── Measured constants ────────────────────────────────────────────────────────

/// `family.ofn`'s measured predictors (2026-08-03, release, this host):
/// 1337 `ObjectPropertyAssertion`, 31 role-chain rules + 7 transitive roles.
/// Hard-coded because the corpus is gitignored; the corpus-gated test below
/// re-derives them from the real file when it is present, so a drift between
/// these numbers and the ontology is detectable.
const FAMILY_EDGES: usize = 1337;
const FAMILY_MULTIPLIERS: usize = 38;

/// `family.ofn`'s measured pre-check cost, in isolation, release profile.
/// The classify-level detection flips between 2600 and 2700 ms.
const FAMILY_PRECHECK_MS: u64 = 2585;

/// The budget this change supersedes. Pinned as a literal on purpose: several
/// assertions below are "never worse than the thing we replaced", and that
/// claim must not silently follow the new constant around.
const SUPERSEDED_FLAT_DEFAULT_MS: u64 = 3000;

/// `ore_ont_16315`'s measured predictors — the *cheapest* ontology measured to
/// blow the budget (68 813 ms of pre-check in isolation), and one of the four
/// that regressed to DNF when the pre-check went unbounded.
const ORE_16315_EDGES: usize = 37_222;
const ORE_16315_MULTIPLIERS: usize = 55;

fn parse(src: &str) -> SetOntology<RcStr> {
    read_ofn(
        &mut Cursor::new(src.as_bytes()),
        ParserConfiguration::default(),
    )
    .expect("parse OFN")
    .0
}

fn internal(src: &str) -> InternalOntology {
    owl_dl_core::convert::convert_ontology(&parse(src)).expect("convert")
}

// Env-mutation plumbing (mirrors `classify_inconsistency_budget.rs`).
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
        // SAFETY: see `set`.
        unsafe {
            match self.prior.take() {
                Some(v) => std::env::set_var(self.key, v),
                None => std::env::remove_var(self.key),
            }
        }
    }
}

// ── 1. The rule must not starve `family.ofn` ──────────────────────────────────

/// `family.ofn` must land in the low-work class. This is the assertion a
/// mutation of either constant has to get past: raising
/// `INCONSISTENCY_WORK_THRESHOLD`'s counterpart below family's proxy, or
/// dropping `INCONSISTENCY_GENEROUS_MS` back toward the flat default, fails
/// here.
#[test]
fn family_predictors_get_generous_budget() {
    let p = AboxCostPredictors {
        asserted_edges: FAMILY_EDGES,
        multiplying_rules: FAMILY_MULTIPLIERS,
    };
    assert!(
        p.work_proxy() <= INCONSISTENCY_WORK_THRESHOLD,
        "family.ofn scores {} — above the {INCONSISTENCY_WORK_THRESHOLD} \
         low-work line, so the one ontology this pre-check exists for would be \
         bounded as if it were pathological",
        p.work_proxy()
    );
    assert_eq!(
        adaptive_classify_inconsistency_budget_ms(p),
        INCONSISTENCY_GENEROUS_MS
    );
}

/// The headroom gate, stated as a ratio against a measured wall rather than as
/// a bare constant: 3× the 2585 ms `family.ofn` actually needs. The flat 3000 ms
/// gave 1.16×, which is what made a 15%-slower host lose the detection.
#[test]
fn family_budget_has_at_least_3x_headroom() {
    let p = AboxCostPredictors {
        asserted_edges: FAMILY_EDGES,
        multiplying_rules: FAMILY_MULTIPLIERS,
    };
    let granted = adaptive_classify_inconsistency_budget_ms(p);
    assert!(
        granted >= 3 * FAMILY_PRECHECK_MS,
        "family.ofn needs {FAMILY_PRECHECK_MS} ms of ABox saturation and is \
         granted only {granted} ms — under 3× headroom a moderately slower host \
         silently loses the inconsistency detection v0.4.11 shipped to provide"
    );
}

/// The other side of the same constant, **added because a sabotage survived**:
/// blowing `INCONSISTENCY_GENEROUS_MS` up to 1 000 000 ms left all the original
/// canaries green. Only the *lower* bound was pinned, so "generous" could have
/// drifted to effectively unbounded — which is the v0.4.8 defect (an unbounded
/// pre-check in front of every classify) re-entering through the low-work door.
///
/// The bound: the generous budget is a **worst-case tax** on a low-work
/// ontology, paid only if its fixpoint really runs that long. The slowest
/// low-work pre-check measured over the whole ABox-bearing ORE population is
/// `ore_ont_16632` at 1627 ms, so 12 000 ms already carries ~7× slack over
/// anything observed; 5× the superseded flat default is the outer limit at which
/// the tax stays smaller than the classify it precedes.
#[test]
fn generous_budget_is_bounded_above() {
    // Read the ceiling through the RULE rather than the constant, so the
    // assertion is not a constant expression clippy can fold away — and so it
    // also catches a third branch appearing that returns something larger.
    let granted = adaptive_classify_inconsistency_budget_ms(AboxCostPredictors {
        asserted_edges: std::hint::black_box(1),
        multiplying_rules: std::hint::black_box(1),
    });
    let ceiling = 5 * SUPERSEDED_FLAT_DEFAULT_MS;
    assert!(
        granted <= ceiling,
        "the low-work branch grants {granted} ms, above the {ceiling} ms \
         ceiling (5× the superseded flat default) — at that size a single \
         low-work ontology with a slow fixpoint reintroduces the v0.4.8 \
         'unbounded pre-check in front of every classify' defect through the \
         low-work door"
    );
}

// ── 2. The direction must not invert ─────────────────────────────────────────

/// The trap this whole design is guarding against: a budget that *grows* with
/// `ABox` size. It reads as the obvious rule and it is backwards. Measured:
/// `ore_ont_4510` carries 114 957 `ObjectPropertyAssertion` and saturates in
/// **134 ms**; `family.ofn` carries 1337 and takes **2585 ms**. So a budget
/// increasing in size starves the small expensive case and subsidises the large
/// cheap one.
///
/// Asserted as monotonicity over the work proxy, which is the only ordering the
/// measurement established.
#[test]
fn budget_is_monotone_non_increasing_in_work() {
    let mut prev = u64::MAX;
    for edges in [0usize, 1, 100, 1_337, 10_000, 37_222, 200_000, 2_800_000] {
        for mult in [0usize, 1, 7, 38, 55, 500] {
            let p = AboxCostPredictors {
                asserted_edges: edges,
                multiplying_rules: mult,
            };
            let b = adaptive_classify_inconsistency_budget_ms(p);
            assert!(
                (INCONSISTENCY_STINGY_MS..=INCONSISTENCY_GENEROUS_MS).contains(&b),
                "budget {b} outside [{INCONSISTENCY_STINGY_MS}, \
                 {INCONSISTENCY_GENEROUS_MS}] at edges={edges} mult={mult}"
            );
        }
    }
    // The ordering itself, over a proxy-sorted sweep.
    for proxy_edges in [0usize, 1, 1_000, 26_315, 100_000, 1_000_000, 5_000_000] {
        let p = AboxCostPredictors {
            asserted_edges: proxy_edges,
            multiplying_rules: 1,
        };
        let b = adaptive_classify_inconsistency_budget_ms(p);
        assert!(
            b <= prev,
            "budget rose from {prev} to {b} as predicted work grew to \
             {proxy_edges} — the rule is inverted; see the measured 4510-vs-family \
             counterexample in this test's doc comment"
        );
        prev = b;
    }
}

// ── 3. The rule must never grant LESS than the flat default ──────────────────

/// One-sided-relaxation invariant. Every ontology outside the low-work class is
/// bounded *exactly* as the flat 3000 ms bounded it, so this change cannot
/// reintroduce the four-ontology DNF regression: no input can produce a smaller
/// budget than the value those four ran under.
#[test]
fn never_below_the_superseded_flat_default() {
    for edges in [0usize, 1, 1_337, 37_222, 114_957, 2_810_396, usize::MAX] {
        for mult in [0usize, 1, 3, 38, 55, usize::MAX] {
            let p = AboxCostPredictors {
                asserted_edges: edges,
                multiplying_rules: mult,
            };
            let b = adaptive_classify_inconsistency_budget_ms(p);
            assert!(
                b >= SUPERSEDED_FLAT_DEFAULT_MS,
                "granted {b} ms at edges={edges} mult={mult} — BELOW the \
                 {SUPERSEDED_FLAT_DEFAULT_MS} ms the four DNF-regression \
                 ontologies were validated under"
            );
        }
    }
    assert_eq!(
        INCONSISTENCY_STINGY_MS, SUPERSEDED_FLAT_DEFAULT_MS,
        "the high-work branch must stay bit-identical to the budget the four \
         regressions were measured under"
    );
}

/// The four regressions, at the one set of predictors that was measured
/// exactly: `ore_ont_16315`. It must get the old default and not a byte more.
#[test]
fn regression_shaped_predictors_get_exactly_the_old_default() {
    let p = AboxCostPredictors {
        asserted_edges: ORE_16315_EDGES,
        multiplying_rules: ORE_16315_MULTIPLIERS,
    };
    assert!(
        p.work_proxy() > INCONSISTENCY_WORK_THRESHOLD,
        "ore_ont_16315 scores {} — inside the low-work class, so it would be \
         granted {INCONSISTENCY_GENEROUS_MS} ms of a fixpoint measured at \
         68 813 ms",
        p.work_proxy()
    );
    assert_eq!(
        adaptive_classify_inconsistency_budget_ms(p),
        SUPERSEDED_FLAT_DEFAULT_MS
    );
}

// ── 4. `multiplying_rules == 0` must not collapse the score ──────────────────

/// Without the `max(1)` in [`AboxCostPredictors::work_proxy`] a chain-free
/// ontology scores 0 no matter how many assertions it carries, so the largest
/// `ABox` in the corpus (`ore_ont_7192`, 2.8 M assertions) would be handed the
/// generous budget. Chain-free saturation is linear, but linear in something
/// large.
#[test]
fn chain_free_but_enormous_abox_is_not_low_work() {
    let p = AboxCostPredictors {
        asserted_edges: 2_800_000,
        multiplying_rules: 0,
    };
    assert_eq!(p.work_proxy(), 2_800_000, "the max(1) guard is gone");
    assert_eq!(
        adaptive_classify_inconsistency_budget_ms(p),
        INCONSISTENCY_STINGY_MS
    );
}

// ── 4b. The prelude-dominated class must stay in the generous branch ─────────

/// The rule is deliberately single-condition, and this pins the reason.
///
/// Extending the population scan from 409 to 1137 ontologies refuted the
/// mechanism model the rule was first built on ("edge multiplication is
/// necessary for expense"): `ore_ont_5368` performs **zero** type and **zero**
/// edge additions and still costs 5936 ms, because the fixpoint's pre-indexing
/// prelude walks all **18.6 M** of its lowered axioms. `ore_ont_1833` is the same
/// shape at 14.1 M axioms / 4478 ms.
///
/// The obvious reflex is to add an axiom-count condition and push them into the
/// stingy branch. Measurement says that would make them **worse**: the prelude
/// runs before the first deadline probe, so their wall is budget-independent —
/// 4065 → 4023 ms and 6059 → 5871 ms going from 3000 ms to 12 000 ms — while
/// `timed_out` flips `true` → `false`. The stingy branch pays their full cost and
/// throws the answer away.
///
/// So: their predictors must keep landing in the generous class. If a future
/// change adds an axiom-count gate, this fails and points at the measurement.
#[test]
fn prelude_dominated_predictors_stay_generous() {
    for (name, edges, mult) in [
        ("ore_ont_1833", 10_865usize, 0usize),
        ("ore_ont_5368", 6_099, 0),
    ] {
        let p = AboxCostPredictors {
            asserted_edges: edges,
            multiplying_rules: mult,
        };
        assert_eq!(
            adaptive_classify_inconsistency_budget_ms(p),
            INCONSISTENCY_GENEROUS_MS,
            "{name} is prelude-dominated, not fixpoint-dominated: its wall does \
             not change with the budget, but the stingy branch discards the \
             verdict it already paid for"
        );
    }
}

// ── 5. The explicit override must still win ──────────────────────────────────

const TINY_ABOX: &str = r"Prefix(:=<http://e.org/>)
Ontology(
  Declaration(Class(:A))
  Declaration(NamedIndividual(:x))
  ClassAssertion(:A :x)
)";

/// `RUSTDL_CLASSIFY_INCONSISTENCY_MS` is the documented escape hatch; the
/// adaptive rule must not shadow it.
#[test]
fn env_override_wins() {
    let _lock = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let onto = internal(TINY_ABOX);
    let _g = SetEnvGuard::set("RUSTDL_CLASSIFY_INCONSISTENCY_MS", "77");
    assert_eq!(classify_inconsistency_budget_ms(&onto), 77);
}

/// `0` means unbounded, and must survive the adaptive layer — a tiny `ABox`
/// would otherwise be silently re-bounded at the generous default.
#[test]
fn env_override_zero_means_unbounded() {
    let _lock = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let onto = internal(TINY_ABOX);
    let _g = SetEnvGuard::set("RUSTDL_CLASSIFY_INCONSISTENCY_MS", "0");
    assert_eq!(
        classify_inconsistency_budget_ms(&onto),
        0,
        "0 must reach the caller as 'unbounded', not be replaced by the \
         adaptive value"
    );
}

/// With no override the adaptive value is what ships.
#[test]
fn no_override_uses_the_adaptive_value() {
    let _lock = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let onto = internal(TINY_ABOX);
    let _g = SetEnvGuard::unset("RUSTDL_CLASSIFY_INCONSISTENCY_MS");
    assert_eq!(
        classify_inconsistency_budget_ms(&onto),
        INCONSISTENCY_GENEROUS_MS,
        "a 1-assertion ABox is the low-work class by construction"
    );
}

// ── 6. The predictor pass must count what it claims ──────────────────────────

/// Every threshold above is calibrated against these two counts. If the pass
/// miscounts — e.g. scores a plain `SubObjectPropertyOf` as a multiplier, or
/// misses `TransitiveObjectProperty` — the calibration is against a different
/// quantity than the one the doc comment names.
#[test]
fn predictors_count_edges_and_multipliers() {
    let src = r"Prefix(:=<http://e.org/>)
Ontology(
  Declaration(ObjectProperty(:r))
  Declaration(ObjectProperty(:s))
  Declaration(ObjectProperty(:t))
  Declaration(NamedIndividual(:a))
  Declaration(NamedIndividual(:b))
  Declaration(NamedIndividual(:c))
  TransitiveObjectProperty(:r)
  SubObjectPropertyOf(ObjectPropertyChain(:r :s) :t)
  SubObjectPropertyOf(:s :t)
  ObjectPropertyAssertion(:r :a :b)
  ObjectPropertyAssertion(:r :b :c)
  ObjectPropertyAssertion(:s :a :c)
)";
    let p = abox_cost_predictors(&internal(src));
    assert_eq!(p.asserted_edges, 3, "3 ObjectPropertyAssertion");
    assert_eq!(
        p.multiplying_rules, 2,
        "1 TransitiveObjectProperty + 1 role CHAIN; the plain \
         SubObjectPropertyOf(:s :t) is a hierarchy edge, not a multiplier"
    );
    assert_eq!(p.work_proxy(), 6);
}

/// A TBox-only ontology scores zero on both counts — the pre-check is
/// `has_abox_axioms`-guarded and never runs, so the value is only ever a
/// don't-care, but it must not be *large* (which would look like the
/// pathological class in a diagnostic).
#[test]
fn tbox_only_scores_zero_edges() {
    let src = r"Prefix(:=<http://e.org/>)
Ontology(
  Declaration(Class(:A))
  Declaration(Class(:B))
  SubClassOf(:A :B)
)";
    let p = abox_cost_predictors(&internal(src));
    assert_eq!(p.asserted_edges, 0);
    assert_eq!(p.work_proxy(), 0);
}

// ── Corpus-gated: the real `family.ofn` ──────────────────────────────────────

/// Re-derives `family.ofn`'s predictors from the real file, so the hard-coded
/// [`FAMILY_EDGES`] / [`FAMILY_MULTIPLIERS`] above cannot drift away from the
/// ontology they claim to describe.
///
/// `#[ignore]`d because `ontologies/real/family.ofn` is **gitignored** (fetched
/// by `./scripts/fetch-real-ontologies.sh`), not because it is slow — it only
/// converts, it does not saturate.
#[test]
#[ignore = "requires ontologies/real/family.ofn (gitignored; ./scripts/fetch-real-ontologies.sh)"]
fn real_family_predictors_match_the_hard_coded_ones() {
    let src = std::fs::read_to_string("../../ontologies/real/family.ofn").expect("family.ofn");
    let p = abox_cost_predictors(&internal(&src));
    assert_eq!(
        (p.asserted_edges, p.multiplying_rules),
        (FAMILY_EDGES, FAMILY_MULTIPLIERS),
        "family.ofn's predictors have drifted from the values the threshold was \
         calibrated against — re-measure before trusting the constants"
    );
    assert_eq!(
        adaptive_classify_inconsistency_budget_ms(p),
        INCONSISTENCY_GENEROUS_MS
    );
}

/// The end-to-end value gate: `classify` on the real `family.ofn`, under the
/// shipped adaptive budget, must report it inconsistent.
///
/// **`#[ignore]`d for TWO stated reasons, both real:** (a) the corpus is
/// gitignored; and (b) **this fixture's pre-check costs 37.9 s in the
/// unoptimized test profile** against ~2.6 s in release, so as an un-ignored
/// test it would measure the build profile and the host rather than the rule.
/// Run it against a release build:
///
/// ```sh
/// cargo test --release -p owl-dl-reasoner --test adaptive_inconsistency_budget \
///     -- --ignored real_family_classify_is_inconsistent
/// ```
#[test]
#[ignore = "corpus-gated (family.ofn is not in git) AND release-only: this \
            fixture's ABox pre-check costs 37.9 s in the unoptimized test \
            profile vs ~2.6 s in release, so in a debug run it would measure \
            the profile, not the rule"]
fn real_family_classify_is_inconsistent() {
    let _lock = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let src = std::fs::read_to_string("../../ontologies/real/family.ofn").expect("family.ofn");
    let onto = parse(&src);
    // Exercise the SHIPPED default, not a value chosen here.
    let _clear = SetEnvGuard::unset("RUSTDL_CLASSIFY_INCONSISTENCY_MS");
    let c = owl_dl_reasoner::classify(&onto).expect("classify succeeds");
    assert!(
        c.stats().inconsistent,
        "family.ofn is inconsistent (HermiT, Konclude and `rustdl consistent` \
         all agree); classify reporting it consistent means the adaptive budget \
         starved the pre-check"
    );
    assert_eq!(c.unsatisfiable_classes().len(), 58);
}
