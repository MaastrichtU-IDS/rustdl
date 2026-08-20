//! Task 8 — the two gates that decide whether an incremental session is
//! trustworthy at all.
//!
//! **Gate 1 (identity).** A session that reaches an axiom set via a seeded
//! random edit script must produce an IRI-identical hierarchy, reported-class
//! set and unsatisfiable set to a from-scratch [`owl_dl_reasoner::classify`]
//! over that same set — at EVERY revision, not just the last.
//!
//! **Gate 2 (round trip).** Add axioms, then retract exactly those. The
//! verdict must return to the original. This is the over-retention detector:
//! anything the ADD path leaves behind that survives into the rebuild shows up
//! here as a phantom class or a phantom subsumption.
//!
//! Both run BUDGET-FREE ([`common::budget_free`]). Byte-identity is a claim
//! about the *algorithm*; with a wall-clock budget armed, `timed_out_pairs`
//! and the per-class unsat probe's default-to-satisfiable-on-timeout make the
//! verdict a function of host speed, from-scratch stops being reproducible
//! against itself, and the gate flakes by construction. [`budgets_are_off`] is
//! the executable proof that they really are off.
//!
//! Every assertion is accompanied, in a comment, by the implementation
//! mutation that makes it fail — the plan's controller ruling C3. The
//! mutations that were actually run are recorded in the task-8 report.
//!
//! ## Scope caveat — read this before trusting a green run
//!
//! Three tracked corpus fixtures (`mie.ofn`, `sulo.ofn`, `pizza.ofn`) are NOT
//! in [`FIXTURES`], for two distinct PRE-EXISTING production defects this gate
//! found. Both are pinned below as `#[ignore]`d reproducers that FAIL today.
//! A green run of this file therefore certifies the session over the
//! DKey-free, debug-assert-free subset of the corpus — not over the corpus.
//!
//! What IS covered: all three branches of `recompute_classification`
//! (`classify_from_closure` on `el-partonomy.ofn`, the hybrid classifier's
//! saturator fast path on `pair_08.ofn` and `derived-overlay.ofn` post-add,
//! its tableau path on `paper5.ofn`), at 12–228 classes.
#![allow(clippy::unwrap_used)]

use horned_owl::model::MutableOntology;
use owl_dl_reasoner::incremental::{AxiomDelta, IncrementalSession};

mod common;
use common::{budget_free, canonical_components, load_ofn, split_axioms, verdict};

/// The fixtures the gate runs over. All tracked; `/ontologies/` is gitignored
/// and a gate that only runs on a developer machine is not a gate.
///
/// * `el-partonomy.ofn` — `FragmentClassification::PureEl`, so the session
///   answers from its RETAINED saturation closure
///   (`classify::classify_from_closure`). That is the only path where the
///   session reuses anything, so a gate that skipped it would not be testing
///   incrementality at all. Carries three unsatisfiable classes, so the
///   elided-`⊥ ⊑ *` re-keying is exercised at scale rather than by one
///   hand-built case.
/// * `derived-overlay.ofn` — carries the two REWRITING derivation passes:
///   union-LHS GCIs (`split_disjunctive_antecedents` consumes them and emits
///   the per-disjunct axioms as `derived`) and a length-3 role chain
///   (`decompose_long_chains`). Added specifically because the first two
///   fixtures let the "drop `refresh_derived` from `commit_addition`" mutation
///   SURVIVE — neither has an overlay that moves between revisions, so the
///   gate could not see a stale one. It is `PureEl` from scratch but goes
///   `Horn` in-session from revision 1 (measured), which is the documented
///   lowering divergence, not a defect — see `incremental.rs::commit_addition`.
/// * `paper5.ofn` — `OutOfFragment`, so the session falls through to the full
///   hybrid classifier. Nominal/enumeration heavy; this is the fixture that
///   caught the `classify_n2`-vs-top-down divergence. It is also the only
///   fixture with a non-empty `defined_sups`, i.e. the only one that reaches
///   the un-disable-able 200 ms sweep budget — hence the `timed_out_pairs`
///   assertion in [`assert_revision`].
/// * `pair_08.ofn` — 807 components / 228 classes / 2 363 subsumptions, a ~9x
///   scale step over the rest, and `Data`-axiom-free so it dodges the `DKey`
///   defect below. `Horn`, so it exercises the saturator-complete fast path
///   inside the hybrid classifier. Marked [`Fixture::heavy`]: bounded harder
///   by default, unbounded under `RUSTDL_GATE_FULL=1`.
///
/// NOT here, and why (see the `#[ignore]`d reproducers at the bottom):
/// * `mie.ofn`, `sulo.ofn` — both mint `DKey` synthetic classes, and
///   `classify::reportable_class_iris` mis-keys the hierarchy whenever a
///   `DKey` id sits below a reported class id.
/// * `pizza.ofn` — `classify` trips a `debug_assert!` in `owl-dl-tableau`.
const FIXTURES: &[Fixture] = &[
    Fixture {
        name: "el-partonomy.ofn",
        pure_el_in_session: true,
        heavy: false,
        slow_consistency: false,
    },
    Fixture {
        name: "derived-overlay.ofn",
        pure_el_in_session: false,
        heavy: false,
        slow_consistency: false,
    },
    Fixture {
        name: "paper5.ofn",
        pure_el_in_session: false,
        heavy: false,
        slow_consistency: true,
    },
    Fixture {
        name: "pair_08.ofn",
        pure_el_in_session: false,
        heavy: true,
        slow_consistency: false,
    },
];

/// A gate fixture plus the one structural fact the gate must pin about it.
struct Fixture {
    name: &'static str,
    /// True iff a SESSION over this fixture answers from the retained
    /// saturation closure — `is_pure_el(session.internal)`, observable from
    /// outside as `stats().fragment == PureEl` on the session's own
    /// classification.
    ///
    /// It is asserted at every revision, because it is the ONLY thing that
    /// distinguishes "the session reused its engine" from "the session
    /// re-derived everything and got the same answer". Without it, deleting
    /// the `is_pure_el` branch from `recompute_classification` — so the
    /// session always runs the full classifier — leaves every other assertion
    /// in this file green.
    ///
    /// NOTE it is a claim about the SESSION, not about from-scratch. On
    /// `derived-overlay.ofn` the two legitimately differ: from-scratch is pure
    /// EL, but after an incremental add the session's IR retains the union-LHS
    /// GCI *alongside* its splits (see the comment at
    /// `incremental.rs::commit_addition`), which puts it out of the EL
    /// fragment. Answers are the contract; lowerings are not.
    pure_el_in_session: bool,
    /// Scale fixture: an order of magnitude bigger than the rest, so the
    /// default run bounds it harder to keep the whole file at a few seconds.
    /// `RUSTDL_GATE_FULL=1` removes every bound. See [`HEAVY_STEPS`] and
    /// [`HEAVY_ROUND_TRIP_AXIOMS`].
    heavy: bool,
    /// True iff a BUDGET-FREE `is_consistent` over this fixture is too slow to
    /// run at every revision, so [`assert_revision`]'s consistency comparison
    /// is skipped unless `RUSTDL_GATE_FULL=1`.
    ///
    /// Only `paper5.ofn` is: it is the one `OutOfFragment` fixture, so
    /// `is_consistent` runs the unbounded hybrid tableau — **10.1 s per call,
    /// measured**, against <1 ms for every other fixture. At two calls a
    /// revision (session + from-scratch oracle) × 3 seeds × ~25 revisions that
    /// is ~25 minutes of debug wall for one `#[test]`, which would in practice
    /// get the whole gate skipped.
    ///
    /// The consistency assertion is NOT weakened by this on the axis it was
    /// added for: `retain_consistency` is a function of `Direction` alone, so
    /// every fixture exercises the same retention rule, and the C1 seam itself
    /// (a component that lowers to nothing) needs data axioms, which the
    /// dedicated regression tests in `tests/incremental_session.rs` cover
    /// directly. What is lost here is only the OUT-OF-FRAGMENT consistency
    /// path, and `RUSTDL_GATE_FULL=1` restores it.
    slow_consistency: bool,
}

/// Seeds. Every assertion names its seed, so a red run replays verbatim.
const SEEDS: &[u64] = &[1, 7, 42];

/// One-axiom-at-a-time revisions checked per (fixture, seed) before the tail
/// of the script goes in as a single delta.
///
/// The gate re-classifies FROM SCRATCH at every revision, so its cost is
/// `revisions × classify`; bounding this keeps the default run near a second.
/// `RUSTDL_GATE_FULL=1` removes the bound and walks the whole script one axiom
/// at a time.
const DEFAULT_STEPS: usize = 24;

/// Gate 1's step budget for a `heavy` fixture. `pair_08.ofn` costs ~60 ms per
/// from-scratch classify against ~0.5 ms for the small fixtures, so it gets a
/// tighter walk; the terminal batch delta still takes it to the full axiom set.
const HEAVY_STEPS: usize = 8;

/// Gate 2's add/remove length for a `heavy` fixture. Every removal rebuilds
/// (P1), which on `pair_08.ofn` is ~29 ms — 269 of them is ~8 s on its own.
/// The round trip's shape is unchanged, only its length.
const HEAVY_ROUND_TRIP_AXIOMS: usize = 30;

fn gate_full() -> bool {
    std::env::var("RUSTDL_GATE_FULL").is_ok_and(|v| v != "0")
}

fn steps(heavy: bool) -> usize {
    match (gate_full(), heavy) {
        (true, _) => usize::MAX,
        (false, true) => HEAVY_STEPS,
        (false, false) => DEFAULT_STEPS,
    }
}

/// Deterministic LCG — no `rand` dependency, and a failing seed is
/// reproducible.
fn lcg(state: &mut u64) -> u64 {
    *state = state
        .wrapping_mul(6_364_136_223_846_793_005)
        .wrapping_add(1_442_695_040_888_963_407);
    *state
}

/// One coin flip from `state`, taken from a HIGH bit.
///
/// Never read the low bits of an LCG. With an odd multiplier and an odd
/// increment, bit 0 satisfies `(a·s + c) mod 2 = (s + 1) mod 2` — period TWO.
/// A `% 2 == 0` split is therefore not a random half at all, it is "even
/// indices vs odd indices" for every seed: seeds 1 and 7 produce a
/// bit-for-bit identical script and seed 42 produces its exact complement, so
/// three seeds explore two points in edit-script space and any bug needing
/// two related axioms in the SAME half is systematically un-probed.
/// `seeds_produce_distinct_scripts` pins that this is fixed.
fn coin(state: &mut u64) -> bool {
    (lcg(state) >> 33) & 1 == 0
}

// ---------------------------------------------------------------------------
// The premises of both gates
// ---------------------------------------------------------------------------

/// The gate compares a session against a from-scratch run. If a from-scratch
/// run is not reproducible against ITSELF, the comparison is noise and every
/// assertion below is decoration. So prove the budgets are off first.
///
/// Three checks, because none is sufficient alone:
///  * the budgets read back as OFF through the reasoner's own accessors —
///    this is the discriminating one, and it is the reason the test is not
///    vacuous;
///  * `timed_out_pairs == 0` and no `undecided_pairs()` — a budget that fires
///    records itself there;
///  * two consecutive from-scratch runs agree byte-for-byte on classes,
///    hierarchy and unsat set — the property the gate actually needs.
///
/// HONEST CALIBRATION: on fixtures this small the last two checks cannot
/// discriminate — a 1 ms label-cache deadline still produces
/// `timed_out_pairs == 0` and a reproducible verdict (measured). They are
/// there for when the fixture list grows. The FIRST check is what makes the
/// mutation `RUSTDL_LABEL_CACHE_TIMEOUT_MS=1` fail (measured: left 1, right
/// 0); it fails identically if `budget_free()` is simply deleted, since the
/// label-cache deadline defaults to 1000 ms.
#[test]
fn budgets_are_off() {
    budget_free();

    // (a) Read the budgets back through the reasoner's OWN accessors, so this
    //     is a statement about the configuration the classifier will actually
    //     use, not about what the test hoped it set. Both default NON-zero
    //     (`label_cache_timeout_ms` to 1000 ms), so deleting `budget_free()`
    //     — or setting either var to anything else — fails here.
    assert_eq!(
        owl_dl_reasoner::label_cache_timeout_ms(),
        0,
        "the per-class label-cache build deadline is armed"
    );
    assert_eq!(
        owl_dl_reasoner::hyper_trust_sat_min_ms(),
        0,
        "the wall-time-thresholded distrust of a wedge NotSubsumed is armed"
    );
    // (b) The remaining budget switches have no public accessor; assert the
    //     environment directly so a silently-dropped entry in
    //     `BUDGET_FREE_ENV` still fails.
    for key in [
        "RUSTDL_LABEL_CACHE_TIMEOUT_MS",
        "RUSTDL_ADAPTIVE_BUDGET",
        "RUSTDL_AGGREGATE_DEADLINE_MS",
        "RUSTDL_HYPER_TRUST_SAT_MIN_MS",
        "RUSTDL_REALIZE_PAIR_TIMEOUT_MS",
    ] {
        assert_eq!(
            std::env::var(key).ok().as_deref(),
            Some("0"),
            "{key} is not pinned off"
        );
    }

    // (c) ... and the property all of that exists to buy.
    for Fixture { name: f, .. } in FIXTURES {
        let o = load_ofn(f);
        let a = owl_dl_reasoner::classify(&o).unwrap();
        assert_eq!(
            a.stats().timed_out_pairs,
            0,
            "{f}: a probe was cut by a budget — this gate cannot be run bounded"
        );
        assert!(
            a.undecided_pairs().is_empty(),
            "{f}: undecided pairs remain: {:?}",
            a.undecided_pairs()
        );
        let b = owl_dl_reasoner::classify(&o).unwrap();
        assert_eq!(
            verdict(&a),
            verdict(&b),
            "{f}: from-scratch is not reproducible against itself"
        );
    }
}

/// The three seeds must name three DIFFERENT edit scripts, or the gate costs
/// 3x and buys 1x and "seed 42" is not a useful thing to print in a failure.
///
/// This is a regression test for a real defect in the first version of this
/// file: it split on `lcg(..) % 2 == 0`, and an LCG's low bit has period two,
/// so seeds 1 and 7 produced IDENTICAL splits and seed 42 produced the exact
/// complement. See [`coin`].
#[test]
fn seeds_produce_distinct_scripts() {
    budget_free();
    for Fixture { name: fixture, .. } in FIXTURES {
        let full = load_ofn(fixture);
        let splits: Vec<Vec<bool>> = SEEDS
            .iter()
            .map(|&seed| {
                let mut state = seed;
                (0..canonical_components(&full).len())
                    .map(|_| coin(&mut state))
                    .collect()
            })
            .collect();
        for (i, a) in splits.iter().enumerate() {
            // Not a constant split (all-in / all-out would leave nothing to add).
            assert!(
                a.iter().any(|&b| b) && a.iter().any(|&b| !b),
                "{fixture} seed {}: the split is constant",
                SEEDS[i]
            );
            for (j, b) in splits.iter().enumerate().skip(i + 1) {
                assert_ne!(
                    a, b,
                    "{fixture}: seeds {} and {} name the SAME script",
                    SEEDS[i], SEEDS[j]
                );
                // ... and not the exact complement of each other either, which
                // is the other way a low-bit LCG degenerates.
                assert!(
                    a.iter().zip(b).any(|(x, y)| x == y),
                    "{fixture}: seed {} is the exact complement of seed {}",
                    SEEDS[j],
                    SEEDS[i]
                );
            }
        }
    }
}

/// The gate's fixtures are green partly BECAUSE they mint no `DKey` class.
/// Pin that, so nobody widens `FIXTURES` with a DKey-bearing ontology and
/// reads the resulting failure as a session regression.
///
/// Delete this test — and fold `mie.ofn` / `sulo.ofn` back into `FIXTURES` —
/// the day `reportable_class_iris` stops aliasing ids.
#[test]
fn gate_fixtures_are_free_of_the_known_dkey_alias_hazard() {
    budget_free();
    for Fixture { name: f, .. } in FIXTURES {
        let internal = owl_dl_core::convert::convert_ontology(&load_ofn(f)).unwrap();
        let dkeys: Vec<usize> = (0..internal.vocabulary.num_classes())
            .filter(|&i| {
                internal
                    .vocabulary
                    .class_iri(owl_dl_core::ClassId::new(u32::try_from(i).unwrap()))
                    .starts_with(owl_dl_core::DKEY_IRI_PREFIX)
            })
            .collect();
        assert!(
            dkeys.is_empty(),
            "{f} mints DKey classes at {dkeys:?} — it is subject to the id-aliasing \
             defect pinned by `known_bug_dkey_ids_alias_reported_classes_from_scratch` \
             and cannot be a gate fixture until that is fixed"
        );
    }
}

/// Assert one revision of a session against a from-scratch run, and pin the
/// two structural facts that make the comparison non-vacuous.
///
/// * `expected.stats().timed_out_pairs == 0` — a budget that fires here would
///   otherwise surface as a mysterious hierarchy divergence rather than as
///   "a budget fired". `classify.rs:2023`'s defined-sup sweep carries a
///   hard-coded 200 ms wall with NO env override, and it is live on
///   `paper5.ofn` (6 `EquivalentClasses` ⇒ non-empty `defined_sups`), so this
///   is the one budget `budget_free()` cannot switch off. There is ~200x
///   headroom today; this line is what turns a future CI-contention firing
///   into a one-line diagnosis.
/// * the session's fragment — see [`Fixture::pure_el_in_session`]. This is
///   the assertion that stops the session from silently re-deriving
///   everything from scratch and still passing.
fn assert_revision(
    fixture: &str,
    seed: u64,
    where_: &str,
    pure_el_in_session: bool,
    current: Option<&horned_owl::ontology::set::SetOntology<horned_owl::model::RcStr>>,
    expected: &owl_dl_reasoner::Classification,
    session: &mut IncrementalSession<horned_owl::model::RcStr>,
) {
    assert_eq!(
        expected.stats().timed_out_pairs,
        0,
        "{fixture} seed {seed} {where_}: a budget fired in the from-scratch run — the \
         comparison below is not reproducible, and any divergence it reports is noise"
    );
    let got = session.classify().unwrap();
    if pure_el_in_session {
        assert_eq!(
            got.stats().fragment,
            owl_dl_reasoner::FragmentClassification::PureEl,
            "{fixture} seed {seed} {where_}: the session left the EL fragment, so it is \
             no longer answering from its RETAINED closure — this fixture exists to cover \
             `classify_from_closure` and now covers nothing"
        );
    }
    assert_eq!(
        verdict(expected),
        verdict(got),
        "fixture {fixture} seed {seed} diverged {where_}"
    );
    // The CONSISTENCY verdict, which nothing above reaches: `classify()` is
    // recomputed from `internal`, but `is_consistent()` can answer from a
    // cache that `apply` decided to RETAIN, and the retention rule is the one
    // piece of the session that can be flatly wrong rather than merely stale.
    // Whole-branch review C1 was exactly that — a `Some(false)` kept across a
    // delta that made the KB consistent — and it survived this whole file
    // because the file never asked the question.
    //
    // Deliberately compared against a from-scratch run rather than against a
    // constant: the point is the session/`classify` identity contract, not
    // which way any fixture happens to fall.
    //
    // Mutation: `retain_consistency(prev, _) = prev` (always retain), or a
    // `Staged::direction()` that reports `Empty` for a delta that moved the
    // mirror.
    if let Some(current) = current {
        assert_eq!(
            owl_dl_reasoner::is_consistent(current).unwrap(),
            session.is_consistent().unwrap(),
            "fixture {fixture} seed {seed} {where_}: the session's CONSISTENCY verdict \
             disagrees with a from-scratch run over the same axiom set"
        );
    }
}

// ---------------------------------------------------------------------------
// Gate 1 — identity at every revision
// ---------------------------------------------------------------------------

#[test]
fn addition_script_matches_from_scratch_at_every_revision() {
    budget_free();
    for &Fixture {
        name: fixture,
        pure_el_in_session,
        heavy,
        slow_consistency,
    } in FIXTURES
    {
        let check_consistency = !slow_consistency || gate_full();
        let max_steps = steps(heavy);
        let full = load_ofn(fixture);
        for &seed in SEEDS {
            let mut state = seed;
            // Start from a random half; add the rest in canonical order.
            let (mut current, rest) = split_axioms(&full, |_| coin(&mut state));
            assert!(
                !rest.is_empty(),
                "{fixture} seed {seed}: the split left nothing to add"
            );

            let mut session = IncrementalSession::new(&current).unwrap();
            assert_revision(
                fixture,
                seed,
                "at revision 0 (the base split)",
                pure_el_in_session,
                check_consistency.then_some(&current),
                &owl_dl_reasoner::classify(&current).unwrap(),
                &mut session,
            );

            let head = rest.len().min(max_steps);
            for ax in &rest[..head] {
                session
                    .apply(&AxiomDelta {
                        added: vec![ax.clone()],
                        removed: vec![],
                    })
                    .unwrap();
                current.insert(ax.clone());

                // Mutations that fail here (both run, see the report):
                //  * drop `refresh_derived` from `commit_addition` — the
                //    derivation passes are whole-ontology fixpoints, so the
                //    overlay is stale the moment the axiom set moves;
                //  * stop clearing `self.classification` at commit — the
                //    session then answers every later revision from the cache
                //    it built at revision 0.
                let expected = owl_dl_reasoner::classify(&current).unwrap();
                let rev = session.revision().0;
                assert_revision(
                    fixture,
                    seed,
                    &format!("at revision {rev}"),
                    pure_el_in_session,
                    check_consistency.then_some(&current),
                    &expected,
                    &mut session,
                );
            }

            // The tail as ONE delta, so the script still ENDS at the full
            // axiom set even when `head` truncated it — and so the batch shape
            // of `convert_delta` is exercised, not only the singleton shape.
            if head < rest.len() {
                let tail: Vec<_> = rest[head..].to_vec();
                session
                    .apply(&AxiomDelta {
                        added: tail.clone(),
                        removed: vec![],
                    })
                    .unwrap();
                for ax in tail {
                    current.insert(ax);
                }
                let expected = owl_dl_reasoner::classify(&current).unwrap();
                let rev = session.revision().0;
                assert_revision(
                    fixture,
                    seed,
                    &format!("at the terminal revision {rev}"),
                    pure_el_in_session,
                    check_consistency.then_some(&current),
                    &expected,
                    &mut session,
                );
            }

            // Fixture guard: the script really did reach the whole ontology.
            // Without it the gate could pass by comparing two runs over a
            // trivially small axiom set.
            assert_eq!(
                canonical_components(&current).len(),
                canonical_components(&full).len(),
                "{fixture} seed {seed}: the script did not reach the full axiom set"
            );
            // ANTI-VACUITY, PER (fixture, seed). Every assertion above is
            // also satisfied by a session that rebuilds from scratch on every
            // single delta — precisely the implementation this phase replaces.
            //
            // Summing this across fixtures (the first version of this test)
            // does NOT work: one fixture that reuses nothing hides behind the
            // others' positive counts. In particular the EL fixture — the only
            // one whose ANSWER depends on the retained closure — could rebuild
            // on every delta with the sum still positive.
            assert!(
                session.stats().additions_reused > 0,
                "{fixture} seed {seed}: not one delta was absorbed by the retained \
                 engine ({:?}) — for this fixture the gate proved only that a \
                 rebuild-every-time session is correct",
                session.stats()
            );
            // ... and the SECOND half of the same hole. `additions_reused`
            // says the engine absorbed the delta; it says nothing about
            // whether the reported ANSWER came from that engine. Deleting the
            // `is_pure_el` arm of `recompute_classification` — so the session
            // re-runs the whole hybrid classifier every time — leaves every
            // other assertion in this file green, including `stats().fragment`
            // and `pure_el_mode`, because the hybrid path's own pure-EL early
            // return calls the same `classify_pure_el` with a freshly
            // saturated (and, by Task 6, equal) closure. `closure_answered` is
            // the only observable that separates them. Measured: without this
            // line the mutation survives the whole file.
            if pure_el_in_session {
                assert!(
                    session.stats().closure_answered > 0,
                    "{fixture} seed {seed}: not one classification was ANSWERED from the \
                     retained closure ({:?}) — the session re-derived everything and got \
                     the right answer, which is not what this fixture is here to prove",
                    session.stats()
                );
            }

            if check_consistency {
                assert_consistency_round_trip(fixture, seed, &mut current, &mut session);
            }
        }
    }
}

/// The ANTI-VACUITY half of [`assert_revision`]'s consistency assertion, and
/// the whole-branch-review-C1 net.
///
/// Every gate fixture is CONSISTENT at every revision of the addition script,
/// so the comparison in `assert_revision` cannot fail for a retention bug: a
/// session that retained `Some(true)` unconditionally would agree with the
/// oracle at every step. Measured — `retain_consistency(prev, _) = prev` walks
/// straight through the whole file.
///
/// So drive the session out of consistency and back. Three fresh components
/// (`gateI` in two disjoint classes) make ANY consistent ontology inconsistent,
/// and retracting them restores it, which exercises both retention directions
/// against a from-scratch oracle on a real fixture:
///
///  * `Some(true)` must NOT survive the addition — mutation:
///    `retain_consistency(prev, _) = prev`, or a `direction()` that reports
///    `Empty`/`Retraction` for an addition. Both make the first assertion see
///    a stale `true`.
///  * `Some(false)` must NOT survive the retraction — same two mutations, and
///    the second assertion sees a stale `false`, which is the FALSE-POSITIVE
///    direction (`inconsistent` entails everything).
fn assert_consistency_round_trip(
    fixture: &str,
    seed: u64,
    current: &mut horned_owl::ontology::set::SetOntology<horned_owl::model::RcStr>,
    session: &mut IncrementalSession<horned_owl::model::RcStr>,
) {
    let b = horned_owl::model::Build::new_rc();
    let clash: Vec<horned_owl::model::AnnotatedComponent<horned_owl::model::RcStr>> = vec![
        horned_owl::model::ClassAssertion {
            ce: b.class("http://rustdl.test/gate#C").into(),
            i: b.named_individual("http://rustdl.test/gate#i").into(),
        }
        .into(),
        horned_owl::model::ClassAssertion {
            ce: b.class("http://rustdl.test/gate#D").into(),
            i: b.named_individual("http://rustdl.test/gate#i").into(),
        }
        .into(),
        horned_owl::model::DisjointClasses(vec![
            b.class("http://rustdl.test/gate#C").into(),
            b.class("http://rustdl.test/gate#D").into(),
        ])
        .into(),
    ];

    assert!(
        session.is_consistent().unwrap(),
        "{fixture} seed {seed}: fixture guard — the script must END consistent, or\
         the round trip below proves nothing"
    );

    session
        .apply(&AxiomDelta {
            added: clash.clone(),
            removed: vec![],
        })
        .unwrap();
    for ac in &clash {
        current.insert(ac.clone());
    }
    assert!(
        !owl_dl_reasoner::is_consistent(current).unwrap(),
        "{fixture} seed {seed}: fixture guard — the from-scratch run must call\
         the clash-extended ontology inconsistent"
    );
    assert!(
        !session.is_consistent().unwrap(),
        "{fixture} seed {seed}: a CONSISTENT verdict survived an addition that\
         removed every model"
    );

    session
        .apply(&AxiomDelta {
            added: vec![],
            removed: clash.clone(),
        })
        .unwrap();
    for ac in &clash {
        current.remove(ac);
    }
    assert!(
        owl_dl_reasoner::is_consistent(current).unwrap(),
        "{fixture} seed {seed}: fixture guard — retracting the clash must restore a model"
    );
    assert!(
        session.is_consistent().unwrap(),
        "{fixture} seed {seed}: an INCONSISTENT verdict survived the retraction\
         that restored a model — a false positive, the one answer this reasoner\
         may never give"
    );
}

// ---------------------------------------------------------------------------
// Gate 2 — round trip
// ---------------------------------------------------------------------------
/// Gate 2's copy of the guard [`assert_revision`] carries for Gate 1.
///
/// The gate runs [`budget_free`], but the `defined_sups` same-tier sweep in
/// `classify_top_down_internal` has a HARD-CODED 200 ms wall with no env
/// override, and `paper5.ofn` is the one fixture that reaches it. On a
/// contended host that budget fires, the two sides of every comparison below
/// stop being functions of the algorithm, and the gate reports "the session
/// diverged" for what is really "a budget fired" — the most expensive possible
/// way to spend a debugging afternoon.
fn assert_no_budget_fired(fixture: &str, where_: &str, c: &owl_dl_reasoner::Classification) {
    assert_eq!(
        c.stats().timed_out_pairs,
        0,
        "{fixture}: a budget fired {where_} — the round-trip comparison is not \
         reproducible, and any divergence it reports is noise"
    );
}

#[test]
fn round_trip_add_then_remove_returns_to_the_original() {
    // Over-retention detector: in P1 the removal rebuilds, so this proves the
    // ADD path left no state that survives into the rebuild.
    //
    // Mutations that fail it:
    //  * prune a retraction by INDEX instead of by value — the premise stays
    //    in `user_axioms`, `refresh_derived` re-derives its consequence, and
    //    the restored hierarchy is a strict superset of `before`;
    //  * drop the live-signature filter in `recompute_classification` — every
    //    class the added axioms introduced stays in `classes()` after they are
    //    retracted, so `before.classes != after.classes` (run, see report).
    budget_free();
    for &Fixture {
        name: fixture,
        heavy,
        ..
    } in FIXTURES
    {
        let full = load_ofn(fixture);
        let (base, every_third) = split_axioms(&full, |i| !i.is_multiple_of(3));
        assert!(!every_third.is_empty(), "{fixture}: nothing to round-trip");
        // Every removal rebuilds in P1, so the heavy fixture round-trips a
        // prefix rather than all 269 of its thirds. Shape unchanged, length
        // bounded; `RUSTDL_GATE_FULL=1` restores the whole set.
        let cap = if heavy && !gate_full() {
            HEAVY_ROUND_TRIP_AXIOMS
        } else {
            usize::MAX
        };
        let rest: Vec<_> = every_third.into_iter().take(cap).collect::<Vec<_>>();

        let mut session = IncrementalSession::new(&base).unwrap();
        let before = {
            let c = session.classify().unwrap();
            assert_no_budget_fired(fixture, "at revision 0", c);
            verdict(c)
        };

        // The mid-point is `base + rest`, computed independently of the
        // session — NOT `full`, which is only the same thing when `cap` did
        // not bite.
        let mut mid = base.clone();
        for ax in &rest {
            mid.insert(ax.clone());
        }
        // Fixture guard: the round trip must actually MOVE the verdict, or a
        // do-nothing implementation would pass.
        let mid_expected = {
            let c = owl_dl_reasoner::classify(&mid).unwrap();
            assert_no_budget_fired(fixture, "in the from-scratch mid-point run", &c);
            verdict(&c)
        };
        assert_ne!(
            before, mid_expected,
            "{fixture}: the round-tripped axioms change nothing — this gate would be vacuous"
        );

        for ax in &rest {
            session
                .apply(&AxiomDelta {
                    added: vec![ax.clone()],
                    removed: vec![],
                })
                .unwrap();
        }
        let mid_got = session.classify().unwrap();
        assert_no_budget_fired(fixture, "in the session at the mid-point", mid_got);
        assert_eq!(
            mid_expected,
            verdict(mid_got),
            "{fixture}: the session did not reach the mid-point verdict"
        );

        for ax in &rest {
            session
                .apply(&AxiomDelta {
                    added: vec![],
                    removed: vec![ax.clone()],
                })
                .unwrap();
        }
        let restored = session.classify().unwrap();
        assert_no_budget_fired(fixture, "in the session after the retractions", restored);
        assert_eq!(
            before,
            verdict(restored),
            "{fixture}: add-then-remove did not return to the original verdict"
        );
        // ... and it agrees with a from-scratch run over the restored set,
        // which `before` alone does not prove (both sides could be equally
        // wrong, e.g. if `before` were itself read from a stale cache).
        let base_expected = owl_dl_reasoner::classify(&base).unwrap();
        assert_no_budget_fired(fixture, "in the from-scratch restored run", &base_expected);
        assert_eq!(
            verdict(&base_expected),
            verdict(session.classify().unwrap()),
            "{fixture}: the restored session disagrees with from-scratch"
        );
    }
}

// ---------------------------------------------------------------------------
// Pinned pre-existing defects. `#[ignore]`d so CI stays green; run with
//   cargo test -p owl-dl-reasoner --test incremental_identity_gate -- --ignored
// Each one FAILS today. They are the gate's findings, kept executable.
// ---------------------------------------------------------------------------

/// **SOUNDNESS (FP ≠ 0) in the public `classify()` — no session involved.**
///
/// `classify::reportable_class_iris` enumerates `0..vocabulary.num_classes()`
/// and then FILTERS OUT the `urn:rustdl-dkey:` synthetics. Every consumer of
/// the resulting vector then reads class `i` back as
/// `ClassId::new(i)` — `classify_pure_el`, the `n²` sweep and the top-down walk
/// alike. Those two are the same class only while every filtered-out `DKey` id
/// sits ABOVE every reported class id. When a `DKey` is minted before some
/// named class is first seen, it does not, and every reported class above the
/// first `DKey` is read off the WRONG row.
///
/// The fixture is `mie.ofn` cut to a deterministic half: `convert_ontology`
/// puts a `DKey` at id 73 with 83 named classes, so positions 73.. are
/// shifted. The oracle is deliberately double: adding a `Declaration` for
/// every class the ontology already mentions is LOGICALLY INERT and must not
/// move the hierarchy, and `owl_dl_reasoner::is_subclass_of` answers by IRI
/// and never touches the aliased index. Both agree the unshifted answer is the
/// right one, so the shifted run is emitting genuine false positives
/// (`Tumour ⊑ BPMeasurement`, …) and genuine misses
/// (`HypertensiveReading ⊑ BPMeasurement`, …).
///
/// The corpus has not caught it because every curated fixture carries a
/// complete set of `Declaration` components, which are lowered first and push
/// the `DKey`s to the top of the id space.
#[test]
#[ignore = "KNOWN BUG: reportable_class_iris aliases class ids past a DKey — FP in classify()"]
fn known_bug_dkey_ids_alias_reported_classes_from_scratch() {
    budget_free();
    let full = load_ofn("mie.ofn");
    let mut state = 1_u64;
    let (half, _) = split_axioms(&full, |_| coin(&mut state));

    let internal = owl_dl_core::convert::convert_ontology(&half).unwrap();
    let n = internal.vocabulary.num_classes();
    let iris: Vec<String> = (0..n)
        .map(|i| {
            internal
                .vocabulary
                .class_iri(owl_dl_core::ClassId::new(u32::try_from(i).unwrap()))
                .to_owned()
        })
        .collect();
    let first_dkey = iris
        .iter()
        .position(|s| s.starts_with(owl_dl_core::DKEY_IRI_PREFIX));
    let named = iris
        .iter()
        .filter(|s| !s.starts_with(owl_dl_core::DKEY_IRI_PREFIX))
        .count();
    assert!(
        first_dkey.is_some_and(|f| f < named),
        "fixture guard: this half must actually put a DKey below a reported class \
         (first DKey at {first_dkey:?}, {named} named classes)"
    );

    // Oracle 1: declaring what is already mentioned is logically inert.
    let build = horned_owl::model::Build::new_rc();
    let mut declared = half.clone();
    for iri in iris
        .iter()
        .filter(|s| !s.starts_with(owl_dl_core::DKEY_IRI_PREFIX))
    {
        declared.insert(horned_owl::model::DeclareClass(build.class(iri.as_str())));
    }
    let plain = verdict(&owl_dl_reasoner::classify(&half).unwrap());
    let with_declarations = verdict(&owl_dl_reasoner::classify(&declared).unwrap());
    assert_eq!(
        plain.classes, with_declarations.classes,
        "fixture guard: the inert declarations must not change the reported class set"
    );

    // Oracle 2: `is_subclass_of` answers by IRI and never touches the index.
    for (sub, sup) in &plain.subsumptions {
        assert!(
            owl_dl_reasoner::is_subclass_of(&half, sub, sup).unwrap(),
            "classify() reported a FALSE POSITIVE the by-IRI oracle rejects: {sub} ⊑ {sup}"
        );
    }
    assert_eq!(
        plain, with_declarations,
        "adding logically inert Declarations changed the hierarchy"
    );
}

/// The same defect reached through a session, which is where it is far more
/// likely: after `convert_delta` interns a class the ids are no longer in IRI
/// order, so a `DKey` minted at revision 0 ends up below a class added later.
///
/// Minimal and self-contained: two axioms plus one delta. `Z ⊑ C` and `Z ⊑ B`
/// are both entailed and both LOST, because `Z`'s reported position resolves
/// to the `DKey`'s row.
#[test]
#[ignore = "KNOWN BUG: same DKey id aliasing, reached through IncrementalSession"]
fn known_bug_dkey_alias_loses_session_entailments() {
    budget_free();
    let b = horned_owl::model::Build::new_rc();
    let mut base: horned_owl::ontology::set::SetOntology<horned_owl::model::RcStr> =
        horned_owl::ontology::set::SetOntology::new_rc();
    // Mints `∃dp.DKey(xsd:integer)`, i.e. a synthetic class in the vocabulary.
    base.insert(horned_owl::model::SubClassOf {
        sub: b.class("http://x/B").into(),
        sup: horned_owl::model::ClassExpression::DataSomeValuesFrom {
            dp: b.data_property("http://x/dp"),
            dr: horned_owl::model::DataRange::Datatype(
                b.datatype("http://www.w3.org/2001/XMLSchema#integer"),
            ),
        },
    });
    base.insert(horned_owl::model::SubClassOf {
        sub: b.class("http://x/C").into(),
        sup: b.class("http://x/B").into(),
    });

    let add = horned_owl::model::SubClassOf {
        sub: b.class("http://x/Z").into(),
        sup: b.class("http://x/C").into(),
    };
    let mut session = IncrementalSession::new(&base).unwrap();
    session
        .apply(&AxiomDelta {
            added: vec![add.clone().into()],
            removed: vec![],
        })
        .unwrap();

    let mut union = base.clone();
    union.insert(add);
    assert_eq!(
        verdict(&owl_dl_reasoner::classify(&union).unwrap()),
        verdict(session.classify().unwrap()),
        "a class interned above a DKey is read off the DKey's row"
    );
}

/// `owl_dl_reasoner::classify(pizza.ofn)` trips a `debug_assert!` in
/// `owl-dl-tableau` — `hyper.rs:3677`, "≤1 violation reached
/// `find_open_at_most` under `inverse_func_merge`" — so the fixture cannot be used
/// by any test that runs on a debug build. Nothing to do with the session:
/// this is a plain from-scratch call. Release builds compile the assertion
/// out, which is why the benchmarks never saw it.
#[test]
#[ignore = "KNOWN BUG: classify(pizza.ofn) trips a tableau debug_assert on debug builds"]
fn known_bug_pizza_trips_a_tableau_debug_assert() {
    budget_free();
    let o = load_ofn("pizza.ofn");
    let _ = owl_dl_reasoner::classify(&o).unwrap();
}
