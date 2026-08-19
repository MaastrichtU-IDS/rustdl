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
///   gate could not see a stale one. See the report.
/// * `paper5.ofn` — `OutOfFragment`, so the session falls through to the full
///   hybrid classifier. Nominal/enumeration heavy; this is the fixture that
///   caught the `classify_n2`-vs-top-down divergence.
///
/// NOT here, and why (see the `#[ignore]`d reproducers at the bottom):
/// * `mie.ofn`, `sulo.ofn` — both mint `DKey` synthetic classes, and
///   `classify::reportable_class_iris` mis-keys the hierarchy whenever a
///   `DKey` id sits below a reported class id.
/// * `pizza.ofn` — `classify` trips a `debug_assert!` in `owl-dl-tableau`.
const FIXTURES: &[&str] = &["el-partonomy.ofn", "derived-overlay.ofn", "paper5.ofn"];

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

fn steps() -> usize {
    if std::env::var("RUSTDL_GATE_FULL").is_ok_and(|v| v != "0") {
        usize::MAX
    } else {
        DEFAULT_STEPS
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
    for f in FIXTURES {
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

/// The gate's fixtures are green partly BECAUSE they mint no `DKey` class.
/// Pin that, so nobody widens `FIXTURES` with a DKey-bearing ontology and
/// reads the resulting failure as a session regression.
///
/// Delete this test — and fold `mie.ofn` / `sulo.ofn` back into `FIXTURES` —
/// the day `reportable_class_iris` stops aliasing ids.
#[test]
fn gate_fixtures_are_free_of_the_known_dkey_alias_hazard() {
    budget_free();
    for f in FIXTURES {
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

// ---------------------------------------------------------------------------
// Gate 1 — identity at every revision
// ---------------------------------------------------------------------------

#[test]
fn addition_script_matches_from_scratch_at_every_revision() {
    budget_free();
    let max_steps = steps();
    let mut total_reused = 0_u64;
    for fixture in FIXTURES {
        let full = load_ofn(fixture);
        for &seed in SEEDS {
            let mut state = seed;
            // Start from a random half; add the rest in canonical order.
            let (mut current, rest) = split_axioms(&full, |_| lcg(&mut state).is_multiple_of(2));
            assert!(
                !rest.is_empty(),
                "{fixture} seed {seed}: the split left nothing to add"
            );

            let mut session = IncrementalSession::new(&current).unwrap();
            assert_eq!(
                verdict(&owl_dl_reasoner::classify(&current).unwrap()),
                verdict(session.classify().unwrap()),
                "fixture {fixture} seed {seed} diverged at revision 0 (the base split)"
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
                assert_eq!(
                    verdict(&expected),
                    verdict(session.classify().unwrap()),
                    "fixture {fixture} seed {seed} diverged at revision {}",
                    session.revision().0
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
                assert_eq!(
                    verdict(&expected),
                    verdict(session.classify().unwrap()),
                    "fixture {fixture} seed {seed} diverged at the terminal revision {}",
                    session.revision().0
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
            total_reused += session.stats().additions_reused;
        }
    }

    // ANTI-VACUITY. Every assertion above is also satisfied by a session that
    // rebuilds from scratch on every single delta — which is precisely the
    // implementation this phase exists to replace. Demand that the retained
    // engine actually absorbed additions, or the gate certifies nothing about
    // incrementality.
    assert!(
        total_reused > 0,
        "no delta in the whole gate was absorbed by the retained engine — the gate \
         proved only that a rebuild-every-time session is correct"
    );
}

// ---------------------------------------------------------------------------
// Gate 2 — round trip
// ---------------------------------------------------------------------------

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
    for fixture in FIXTURES {
        let full = load_ofn(fixture);
        let (base, rest) = split_axioms(&full, |i| i % 3 != 0);
        assert!(!rest.is_empty(), "{fixture}: nothing to round-trip");

        let mut session = IncrementalSession::new(&base).unwrap();
        let before = verdict(session.classify().unwrap());

        // Fixture guard: the round trip must actually MOVE the verdict, or a
        // do-nothing implementation would pass.
        let mid_expected = verdict(&owl_dl_reasoner::classify(&full).unwrap());
        assert_ne!(
            before, mid_expected,
            "{fixture}: the removed third changes nothing — this gate would be vacuous"
        );

        for ax in &rest {
            session
                .apply(&AxiomDelta {
                    added: vec![ax.clone()],
                    removed: vec![],
                })
                .unwrap();
        }
        assert_eq!(
            mid_expected,
            verdict(session.classify().unwrap()),
            "{fixture}: the session did not reach the full ontology's verdict"
        );

        for ax in &rest {
            session
                .apply(&AxiomDelta {
                    added: vec![],
                    removed: vec![ax.clone()],
                })
                .unwrap();
        }
        assert_eq!(
            before,
            verdict(session.classify().unwrap()),
            "{fixture}: add-then-remove did not return to the original verdict"
        );
        // ... and it agrees with a from-scratch run over the restored set,
        // which `before` alone does not prove (both sides could be equally
        // wrong, e.g. if `before` were itself read from a stale cache).
        assert_eq!(
            verdict(&owl_dl_reasoner::classify(&base).unwrap()),
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
    let (half, _) = split_axioms(&full, |_| lcg(&mut state).is_multiple_of(2));

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
