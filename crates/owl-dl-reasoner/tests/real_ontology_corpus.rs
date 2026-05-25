//! Real-ontology feature tests. Each test loads a known-shape
//! ontology from `tests/fixtures/` or `ontologies/real/`, classifies
//! it, and asserts that the unsatisfiable-class set matches the
//! reference (HermiT-via-ROBOT) verdict captured at the comment site.
//!
//! These would have caught the soundness bugs landed 2026-05-25 —
//! none of the 87 in-tree fixtures exercised the (Functional + Equiv
//! + ∃) / (Equiv with disjunctive intersection) interactions that
//! produced rustdl's false-positive unsats on pizza. They run on
//! every `cargo test` so future deps-tracking regressions in the
//! merge / branch / absorption paths surface immediately.
//!
//! `pizza.ofn` and the in-repo fixtures are the *small* workloads
//! that finish in well under a second per probe; SULO and SIO live
//! in `ontologies/real/` (gitignored) and are exercised only when
//! the corpus is present, via `#[ignore]`d siblings.

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::{classify, classify_top_down_with_timeout, classify_with_timeout};
use std::collections::BTreeSet;
use std::fs;
use std::io::Cursor;
use std::path::Path;
use std::time::Duration;

/// Load an `.ofn` file from a path and return a `SetOntology`.
fn load(path: &Path) -> SetOntology<RcStr> {
    let src = fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let mut reader = Cursor::new(src);
    let (ontology, _prefixes) =
        read(&mut reader, ParserConfiguration::default()).expect("ontology parses");
    ontology
}

/// Classify, return the sorted set of unsatisfiable-class IRIs.
fn unsat_set(onto: &SetOntology<RcStr>) -> BTreeSet<String> {
    let c = classify(onto).expect("classify returns Ok");
    c.unsatisfiable_classes().iter().map(|s| s.to_string()).collect()
}

/// Same as `unsat_set` but with a per-pair tableau deadline — sound
/// under-approximation: timed-out probes default to satisfiable, so
/// this lower-bound the unsat set vs. the unbounded version.
fn unsat_set_timed(onto: &SetOntology<RcStr>, per_pair_ms: u64) -> BTreeSet<String> {
    let c = classify_with_timeout(onto, Duration::from_millis(per_pair_ms))
        .expect("classify_with_timeout returns Ok");
    c.unsatisfiable_classes().iter().map(|s| s.to_string()).collect()
}

/// Top-down classifier with per-pair deadline. The default O(n²)
/// pair-loop classifier doesn't finish on workloads with thousands
/// of classes (SIO has 1585); the top-down path walks the partial
/// hierarchy in O(n · depth · branching) tableau calls.
fn unsat_set_top_down(onto: &SetOntology<RcStr>, per_pair_ms: u64) -> BTreeSet<String> {
    let c = classify_top_down_with_timeout(onto, Duration::from_millis(per_pair_ms))
        .expect("classify_top_down_with_timeout returns Ok");
    c.unsatisfiable_classes().iter().map(|s| s.to_string()).collect()
}

/// Pizza (full, raw): HermiT (via ROBOT v1.9.6) reports exactly two
/// unsatisfiable classes:
///   - http://www.co-ode.org/ontologies/pizza/pizza.owl#CheeseyVegetableTopping
///   - http://www.co-ode.org/ontologies/pizza/pizza.owl#IceCream
///
/// rustdl currently reports a *superset* (≈20 false positives on top
/// of the two true unsats — the named-pizza bug noted in
/// `docs/perf-2026-05-24-new-server.md` §5). This test pins the two
/// classes that *must* be in the set; the should-not-be-unsat side
/// of the comparison is captured below with `#[ignore]` until that
/// bug is fixed.
#[test]
#[cfg_attr(not(feature = "real-corpus"), ignore = "needs ontologies/real/pizza.ofn (gitignored corpus)")]
fn pizza_unsat_includes_hermit_truth() {
    let path = Path::new("../../ontologies/real/pizza.ofn");
    if !path.exists() {
        eprintln!("skip: {} not present", path.display());
        return;
    }
    let onto = load(path);
    let unsat = unsat_set_timed(&onto, 200);
    let expected_true_unsats = [
        "http://www.co-ode.org/ontologies/pizza/pizza.owl#CheeseyVegetableTopping",
        "http://www.co-ode.org/ontologies/pizza/pizza.owl#IceCream",
    ];
    for c in expected_true_unsats {
        assert!(
            unsat.contains(c),
            "pizza unsat set is missing {c}; got {} classes",
            unsat.len()
        );
    }
}

/// Pizza (full, raw): HermiT says these toppings are *satisfiable*.
/// They were the false-positive unsats traced to the merge/branch
/// deps-loss bugs fixed 2026-05-25 — the regression test for the
/// underlying patterns lives in `crates/owl-dl-reasoner/src/lib.rs`,
/// but pinning the end-to-end verdicts here means future bugs in the
/// same area surface against the real ontology, not just the
/// minimal repros.
#[test]
#[cfg_attr(not(feature = "real-corpus"), ignore = "needs ontologies/real/pizza.ofn (gitignored corpus)")]
fn pizza_known_satisfiable_toppings() {
    let path = Path::new("../../ontologies/real/pizza.ofn");
    if !path.exists() {
        eprintln!("skip: {} not present", path.display());
        return;
    }
    let onto = load(path);
    let unsat = unsat_set_timed(&onto, 200);
    let must_be_sat = [
        "http://www.co-ode.org/ontologies/pizza/pizza.owl#AsparagusTopping",
        "http://www.co-ode.org/ontologies/pizza/pizza.owl#AnchoviesTopping",
        "http://www.co-ode.org/ontologies/pizza/pizza.owl#HamTopping",
        "http://www.co-ode.org/ontologies/pizza/pizza.owl#MozzarellaTopping",
        "http://www.co-ode.org/ontologies/pizza/pizza.owl#TomatoTopping",
    ];
    for c in must_be_sat {
        assert!(
            !unsat.contains(c),
            "pizza marked {c} as unsat — soundness regression vs. HermiT"
        );
    }
}

/// Pizza (full, raw): HermiT reports *exactly* two unsat classes.
/// This is the strict end-to-end test: rustdl's verdict set must
/// equal HermiT's. The four 2026-05-25 deps-tracking fixes plus the
/// branching-strategy reorder of disjuncts (try leaf-compound
/// before atomic before expensive) made this pass on the
/// `--features real-corpus` build (pizza classify is now ~58 s wall
/// with `--pair-timeout-ms 200`).
#[test]
#[cfg_attr(not(feature = "real-corpus"), ignore = "needs ontologies/real/pizza.ofn (gitignored corpus)")]
fn pizza_unsat_matches_hermit_exactly() {
    let path = Path::new("../../ontologies/real/pizza.ofn");
    if !path.exists() {
        eprintln!("skip: {} not present", path.display());
        return;
    }
    let onto = load(path);
    let unsat = unsat_set_timed(&onto, 200);
    let expected: BTreeSet<String> = [
        "http://www.co-ode.org/ontologies/pizza/pizza.owl#CheeseyVegetableTopping",
        "http://www.co-ode.org/ontologies/pizza/pizza.owl#IceCream",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    assert_eq!(
        unsat, expected,
        "pizza unsat set ({} classes) differs from HermiT ({} classes); extras: {:?}; missing: {:?}",
        unsat.len(),
        expected.len(),
        unsat.difference(&expected).collect::<Vec<_>>(),
        expected.difference(&unsat).collect::<Vec<_>>(),
    );
}

/// SULO (stripped, data-property axioms removed via ROBOT):
/// HermiT reports zero unsat classes. Should hold for rustdl too.
#[test]
#[cfg_attr(not(feature = "real-corpus"), ignore = "needs ontologies/real/sulo-stripped.ofn")]
fn sulo_no_false_unsat() {
    let path = Path::new("../../ontologies/real/sulo-stripped.ofn");
    if !path.exists() {
        eprintln!("skip: {} not present", path.display());
        return;
    }
    let onto = load(path);
    let unsat = unsat_set_timed(&onto, 200);
    assert!(
        unsat.is_empty(),
        "SULO marked {} classes as unsat — HermiT reports zero: {:?}",
        unsat.len(),
        unsat,
    );
}

/// The 5-axiom regression for the merge-deps bug, end-to-end.
/// Mirrors the unit test in `lib.rs` but anchors at the file form
/// so the OFN parser also gets exercised. HermiT says `:A` is sat.
#[test]
fn functional_equiv_some_fixture_is_sat() {
    let onto = load(Path::new("tests/fixtures/functional-equiv-some-bug.ofn"));
    let unsat = unsat_set(&onto);
    assert!(
        !unsat.contains("http://example.org/A"),
        "fixture flagged :A unsat — merge-into deps regression. unsat set: {unsat:?}",
    );
}

// ----------------------------------------------------------------------
// SIO (1585 classes, SROIQ — semanticscience integrated ontology)
//
// HermiT (via ROBOT v1.9.6 docker) classifies SIO in ~200 s wall and
// reports **zero unsatisfiable classes** — `sio-stripped.ofn` is
// fully satisfiable. rustdl's default pair-loop classifier doesn't
// finish (N² = ~2.5 M pair tests at 200 ms each), but the top-down
// classifier walks the partial hierarchy and currently finishes in
// ~272 s with **22 false-positive unsats** clustered tightly:
//   SIO_000450..000463, SIO_000521..000524, SIO_000902..000903,
//   SIO_001173, SIO_001178.
// These cluster around the
// "two `ObjectExactCardinality(1, R, X)` constraints + an inverse-
// role existential" shape on `directed line segment` ⊏ `geometric
// entity`. It's a different SROIQ-rule-interaction bug class than
// the 2026-05-25 pizza fixes addressed (apply_min / apply_max
// cardinality merging through inverse roles). See
// `docs/perf-2026-05-24-new-server.md` for the bisection notes.

/// SIO classify under `--top-down` finishes. No soundness claim
/// here — just that the run terminates within a generous wall
/// budget. This was a *new* property as of the
/// 2026-05-25 cycle-guard + branching-reorder work; before, the
/// N² pair-loop classifier ran out of patience on 1585 classes.
#[test]
#[cfg_attr(not(feature = "real-corpus"), ignore = "needs ontologies/real/sio-stripped.ofn")]
fn sio_top_down_finishes() {
    let path = Path::new("../../ontologies/real/sio-stripped.ofn");
    if !path.exists() {
        eprintln!("skip: {} not present", path.display());
        return;
    }
    let onto = load(path);
    let start = std::time::Instant::now();
    let _ = unsat_set_top_down(&onto, 200);
    let wall = start.elapsed();
    // Generous: HermiT does it in ~200 s; rustdl in ~272 s. The cap
    // exists only to flag a *regression to a hang*, not to track
    // perf at second-level granularity (variance is high).
    assert!(
        wall < Duration::from_secs(20 * 60),
        "SIO top-down classify took {wall:?} — perf regression vs ~272 s baseline",
    );
}

/// SIO classify reports exactly the HermiT-correct unsat set — zero
/// unsatisfiable classes. Used to be 22 false-positives traced to
/// `apply_min` over-asserting pairwise distinctness on every
/// firing (including witnesses brought in by subclass propagation
/// e.g. `:X508 ⊑ :X532` adding `:X532` to a node that was already
/// a `Min(2, :r, :X532)` witness). The fix limits pairwise marking
/// to the first `n` witnesses — see `apply_min` in
/// `crates/owl-dl-tableau/src/rules.rs`.
#[test]
#[cfg_attr(not(feature = "real-corpus"), ignore = "needs ontologies/real/sio-stripped.ofn")]
fn sio_matches_hermit_exactly() {
    let path = Path::new("../../ontologies/real/sio-stripped.ofn");
    if !path.exists() {
        eprintln!("skip: {} not present", path.display());
        return;
    }
    let onto = load(path);
    let unsat = unsat_set_top_down(&onto, 200);
    assert!(
        unsat.is_empty(),
        "SIO unsat count {} (HermiT says 0); false-positives: {:?}",
        unsat.len(),
        unsat,
    );
}

// ----------------------------------------------------------------------
// Family (58 classes, SROIQ — Robert Stevens' family ontology)
//
// The original family.rdf.owl uses several **length-3** role-chain
// axioms for cousins / great-relatives, e.g.
//   ObjectPropertyChain(:hasParent :isSiblingOf :isParentOf) ⊑ :isFirstCousinOf
// rustdl's chain rule supports length-2 chains; longer chains are
// silently *skipped* (see `collect_chain_axioms` in
// `crates/owl-dl-reasoner/src/lib.rs`). The skip under-approximates
// the role-side closure but is sound for class-side reasoning: the
// long-chain super-roles only appear in role-axiom declarations
// (Domain / Range / Symmetric / SubObjectPropertyOf), never in any
// class definition. So the class-hierarchy classification matches
// HermiT exactly.

/// Family-stripped classify matches HermiT — zero unsat — under the
/// length-N chain skip. Top-down classifier finishes in ~8.7 s vs
/// HermiT's ~42 s.
#[test]
#[cfg_attr(not(feature = "real-corpus"), ignore = "needs ontologies/real/family-stripped.ofn")]
fn family_no_false_unsat() {
    let path = Path::new("../../ontologies/real/family-stripped.ofn");
    if !path.exists() {
        eprintln!("skip: {} not present", path.display());
        return;
    }
    let onto = load(path);
    let unsat = unsat_set_top_down(&onto, 200);
    assert!(
        unsat.is_empty(),
        "family marked {} classes as unsat — HermiT reports zero: {:?}",
        unsat.len(),
        unsat,
    );
}

/// `:SIO_000450` ("axis") is the lexicographically-first member of
/// the now-fixed false-unsat cluster (450–463, 521–524, 902–903,
/// 1173, 1178). HermiT classifies it sat; rustdl does too post-fix.
#[test]
#[cfg_attr(not(feature = "real-corpus"), ignore = "needs ontologies/real/sio-stripped.ofn")]
fn sio_axis_is_sat() {
    let path = Path::new("../../ontologies/real/sio-stripped.ofn");
    if !path.exists() {
        eprintln!("skip: {} not present", path.display());
        return;
    }
    let onto = load(path);
    let unsat = unsat_set_top_down(&onto, 200);
    let axis = "http://semanticscience.org/resource/SIO_000450";
    assert!(
        !unsat.contains(axis),
        "SIO_000450 should be sat (matches HermiT); regression of the apply_min over-assert bug",
    );
}