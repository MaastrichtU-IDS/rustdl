//! Closure diff against Konclude's classified output.
//!
//! For each corpus ontology where we have a pre-computed Konclude
//! classification (`*-classified.owx`), classify it with rustdl and
//! compute:
//!
//! - `rustdl_closure` — the number of atomic-class (sub, sup) pairs in
//!   rustdl's hierarchy (transitive closure, excluding reflexive and
//!   excluding Thing on either side).
//! - `konclude_closure` — same metric over Konclude's classified.owx.
//! - `FP` — pairs in rustdl's closure but not Konclude's. Soundness
//!   indicator: this MUST be zero.
//! - `MISSED` — pairs in Konclude's closure but not rustdl's.
//!   Completeness indicator: lower is better.
//!
//! Each test is `#[ignore]`d and runs only when invoked explicitly via
//! `cargo test ... -- --ignored`. They reproduce the FP/MISSED metric
//! the previous session used when chasing the SIO unsoundness — and
//! validate that the sound range encoding (Tseitin-folded existential
//! body) doesn't regress FP=0 on any corpus.

#![allow(clippy::doc_markdown)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::io::owx::reader::read as read_owx;
use horned_owl::model::{ClassExpression, Component, EquivalentClasses, RcStr, SubClassOf};
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::{Classification, classify_top_down_with_timeout};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::{BufReader, Cursor};
use std::path::Path;
use std::time::{Duration, Instant};

const OWL_THING: &str = "http://www.w3.org/2002/07/owl#Thing";
const OWL_NOTHING: &str = "http://www.w3.org/2002/07/owl#Nothing";

/// Shorthand for the (sub, sup) IRI-string pair sets used throughout the
/// closure-diff helpers and the anytime sweep.
type PairSet = BTreeSet<(String, String)>;

/// Parsed Konclude verdict: direct subsumption edges (atomic-only)
/// and the set of classes Konclude proved unsatisfiable (members of
/// some `EquivalentClasses(owl:Nothing, ...)` group).
struct KoncludeVerdict {
    /// (sub, sup) atomic-class direct edges from `SubClassOf` axioms.
    /// Excludes anything involving owl:Thing or owl:Nothing.
    edges: BTreeSet<(String, String)>,
    /// Members of `EquivalentClasses(owl:Nothing, ...)`. Excluded from
    /// the pair-wise comparison.
    unsat: BTreeSet<String>,
    /// Members of `EquivalentClasses(owl:Thing, ...)` — Thing-equivalent
    /// classes. They're trivially supersets of every other class, and
    /// every other class is a subset of them. Konclude omits these
    /// pairs from its output; rustdl correctly derives them. Treating
    /// them like owl:Thing keeps the comparison apples-to-apples.
    /// (E.g., SIO has `EquivalentClasses(owl:Thing, SIO_000000)`.)
    thing_equiv: BTreeSet<String>,
}

/// Read an `.owx` (OWL/XML) ontology and extract the bits we need to
/// compare against rustdl: direct atomic subsumption edges + the set
/// of unsat classes. `EquivalentClasses(X1, ..., Xn)` groups (other
/// than the unsat group ≡ owl:Nothing) are decomposed into a star of
/// bidirectional edges so they're properly included in the closure.
fn read_konclude_verdict(path: &Path) -> KoncludeVerdict {
    let file = File::open(path).unwrap_or_else(|e| panic!("open {}: {e}", path.display()));
    let mut reader = BufReader::new(file);
    let (onto, _): (SetOntology<RcStr>, _) = read_owx(&mut reader, ParserConfiguration::default())
        .unwrap_or_else(|e| panic!("parse {}: {e}", path.display()));
    let mut edges = BTreeSet::new();
    let mut unsat = BTreeSet::new();
    let mut thing_equiv = BTreeSet::new();
    for ax in &onto {
        match &ax.component {
            Component::SubClassOf(SubClassOf { sub, sup }) => {
                if let (ClassExpression::Class(sub_c), ClassExpression::Class(sup_c)) = (sub, sup) {
                    let s = sub_c.0.to_string();
                    let t = sup_c.0.to_string();
                    if s == OWL_THING || t == OWL_THING || s == OWL_NOTHING || t == OWL_NOTHING {
                        continue;
                    }
                    if s != t {
                        edges.insert((s, t));
                    }
                }
            }
            Component::EquivalentClasses(EquivalentClasses(members)) => {
                let iris: Vec<String> = members
                    .iter()
                    .filter_map(|ce| match ce {
                        ClassExpression::Class(c) => Some(c.0.to_string()),
                        _ => None,
                    })
                    .collect();
                let has_nothing = iris.iter().any(|i| i == OWL_NOTHING);
                let has_thing = iris.iter().any(|i| i == OWL_THING);
                if has_nothing {
                    for iri in &iris {
                        if iri != OWL_NOTHING {
                            unsat.insert(iri.clone());
                        }
                    }
                    continue;
                }
                if has_thing {
                    for iri in &iris {
                        if iri != OWL_THING {
                            thing_equiv.insert(iri.clone());
                        }
                    }
                    continue;
                }
                // Non-unsat equivalence group: expand to bidirectional
                // edges so the closure correctly includes both
                // directions and any chain through the group.
                for a in &iris {
                    for b in &iris {
                        if a != b && a != OWL_THING && b != OWL_THING {
                            edges.insert((a.clone(), b.clone()));
                        }
                    }
                }
            }
            _ => {}
        }
    }
    KoncludeVerdict {
        edges,
        unsat,
        thing_equiv,
    }
}

/// Compute the transitive closure of a direct-edge set, excluding
/// reflexive pairs. The corpus closures are small enough (< 50k edges)
/// that naive Warshall over BTreeMap suffices.
fn transitive_closure(edges: &BTreeSet<(String, String)>) -> BTreeSet<(String, String)> {
    let mut succ: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (s, t) in edges {
        succ.entry(s.clone()).or_default().insert(t.clone());
    }
    let mut changed = true;
    while changed {
        changed = false;
        let snapshot: Vec<(String, Vec<String>)> = succ
            .iter()
            .map(|(k, v)| (k.clone(), v.iter().cloned().collect()))
            .collect();
        for (s, ts) in snapshot {
            let mut to_add: Vec<String> = Vec::new();
            for t in &ts {
                if let Some(t_succs) = succ.get(t) {
                    for u in t_succs {
                        if u != &s && !ts.contains(u) {
                            to_add.push(u.clone());
                        }
                    }
                }
            }
            if !to_add.is_empty() {
                let entry = succ.entry(s).or_default();
                for u in to_add {
                    if entry.insert(u) {
                        changed = true;
                    }
                }
            }
        }
    }
    let mut out = BTreeSet::new();
    for (s, ts) in succ {
        for t in ts {
            out.insert((s.clone(), t));
        }
    }
    out
}

/// Convert a `Classification` into a (sub, sup) closure set over
/// **satisfiable** atomic classes (no Thing/Nothing, no unsat-class
/// from either side, no reflexive pairs). Uses `is_subclass` for every
/// ordered pair — O(n²) is fine for corpus sizes (pizza n=100, SIO
/// n≈1500).
fn closure_from_classification(
    c: &Classification,
    exclude: &BTreeSet<String>,
) -> BTreeSet<(String, String)> {
    let rustdl_unsat: BTreeSet<&str> = c.unsatisfiable_classes().iter().copied().collect();
    let classes: Vec<&str> = c
        .classes()
        .iter()
        .map(String::as_str)
        .filter(|s| {
            *s != OWL_THING
                && *s != OWL_NOTHING
                && !exclude.contains(*s)
                && !rustdl_unsat.contains(*s)
        })
        .collect();
    let mut out = BTreeSet::new();
    for &s in &classes {
        for &t in &classes {
            if s == t {
                continue;
            }
            if c.is_subclass(s, t) {
                out.insert((s.to_string(), t.to_string()));
            }
        }
    }
    out
}

/// Per-pair tableau/wedge budget for corpus closure-diffs. Override with
/// `RUSTDL_TEST_PAIR_MS` to sweep the timeout (e.g. measuring whether a low
/// budget introduces MISSED vs ground truth); defaults to 200 ms.
fn test_pair_ms() -> u64 {
    std::env::var("RUSTDL_TEST_PAIR_MS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(200)
}

/// Load an `.ofn` (OWL Functional Syntax) ontology from `path`.
fn load_ofn_fixture(input: &Path) -> SetOntology<RcStr> {
    let src =
        std::fs::read_to_string(input).unwrap_or_else(|e| panic!("read {}: {e}", input.display()));
    let mut reader = Cursor::new(src);
    let (onto, _): (SetOntology<RcStr>, _) = read_ofn(&mut reader, ParserConfiguration::default())
        .unwrap_or_else(|e| panic!("parse {}: {e}", input.display()));
    onto
}

/// Given a `Classification` result and the matching `KoncludeVerdict`, return
/// `(rustdl_pairs, konclude_pairs)` with the same symmetric exclude-set applied
/// to both sides: `exclude = verdict.unsat ∪ rustdl_unsat ∪ verdict.thing_equiv`.
///
/// This is the single canonical alignment definition shared by the closure-diff
/// tests and the anytime sweep — keeping both callers on the same code path
/// ensures FP=0 comparisons are valid (e.g. SIO's `EquivalentClasses(owl:Thing,
/// SIO_000000)` causes spurious FPs without `thing_equiv` exclusion).
fn aligned_closures(c: &Classification, verdict: &KoncludeVerdict) -> (PairSet, PairSet) {
    let rustdl_unsat: BTreeSet<String> = c
        .unsatisfiable_classes()
        .iter()
        .map(ToString::to_string)
        .collect();
    let mut exclude: BTreeSet<String> = verdict.unsat.union(&rustdl_unsat).cloned().collect();
    exclude.extend(verdict.thing_equiv.iter().cloned());
    let rustdl = closure_from_classification(c, &exclude);
    let konclude_full = transitive_closure(&verdict.edges);
    let konclude: BTreeSet<(String, String)> = konclude_full
        .into_iter()
        .filter(|(s, t)| !exclude.contains(s) && !exclude.contains(t))
        .collect();
    (rustdl, konclude)
}

/// Print and return (rustdl_closure, konclude_closure, fp, missed).
fn diff_corpus_ontology(
    label: &str,
    input: &Path,
    truth: &Path,
    per_pair_ms: u64,
) -> (usize, usize, usize, usize) {
    let onto = load_ofn_fixture(input);
    let start = Instant::now();
    let c = classify_top_down_with_timeout(&onto, Duration::from_millis(per_pair_ms))
        .expect("classify");
    let wall = start.elapsed();
    let verdict = read_konclude_verdict(truth);
    let rustdl_unsat_count = c.unsatisfiable_classes().len();
    let (rustdl, konclude) = aligned_closures(&c, &verdict);
    let fp: BTreeSet<_> = rustdl.difference(&konclude).cloned().collect();
    let missed: BTreeSet<_> = konclude.difference(&rustdl).cloned().collect();
    eprintln!(
        "--- {label} ({:.2} s) ---\nrustdl_closure={} konclude_closure={} FP={} MISSED={} (unsat: rustdl={} konclude={} thing-equiv: {})",
        wall.as_secs_f64(),
        rustdl.len(),
        konclude.len(),
        fp.len(),
        missed.len(),
        rustdl_unsat_count,
        verdict.unsat.len(),
        verdict.thing_equiv.len(),
    );
    for (s, t) in fp.iter().take(5) {
        eprintln!(" FP: {s} ⊑ {t}");
    }
    // Print all MISSED when ≤ 50, otherwise first 50.
    let missed_limit = if missed.len() <= 50 { missed.len() } else { 50 };
    for (s, t) in missed.iter().take(missed_limit) {
        eprintln!(" MISSED: {s} ⊑ {t}");
    }
    (rustdl.len(), konclude.len(), fp.len(), missed.len())
}

#[test]
#[ignore = "GALEN with 5 s per-pair timeout — measures how many MISSED are calculus-bound vs timeout-bound"]
fn galen_closure_long_timeout() {
    let input = Path::new("../../ontologies/external/galen.ofn");
    let truth = Path::new("../../ontologies/external/galen-classified.owx");
    if !input.exists() || !truth.exists() {
        eprintln!("SKIP: missing GALEN fixture");
        return;
    }
    let (_r, _k, fp, _m) = diff_corpus_ontology("galen-5s", input, truth, 5000);
    assert_eq!(fp, 0, "GALEN has FPs under 5s timeout");
}

#[test]
#[ignore = "needs ontologies/external/galen.ofn + galen-classified.owx; ~2-12 min wall depending on RUSTDL_HYPERTABLEAU env vars"]
fn galen_closure_matches_konclude() {
    let input = Path::new("../../ontologies/external/galen.ofn");
    let truth = Path::new("../../ontologies/external/galen-classified.owx");
    if !input.exists() || !truth.exists() {
        eprintln!("SKIP: missing GALEN fixture");
        return;
    }
    let (_r, _k, fp, _m) = diff_corpus_ontology("galen", input, truth, 200);
    assert_eq!(fp, 0, "GALEN has FPs — soundness regression");
}

#[test]
#[ignore = "needs ontologies/external/alehif-test.ofn + alehif-test-classified.owx"]
fn alehif_closure_matches_konclude() {
    let input = Path::new("../../ontologies/external/alehif-test.ofn");
    let truth = Path::new("../../ontologies/external/alehif-test-classified.owx");
    if !input.exists() || !truth.exists() {
        eprintln!("SKIP: missing alehif fixture");
        return;
    }
    let (_r, _k, fp, _m) = diff_corpus_ontology("alehif-test", input, truth, 200);
    assert_eq!(fp, 0, "alehif has FPs — soundness regression");
}

#[test]
#[ignore = "needs ontologies/external/notgalen.ofn + notgalen-classified.owx; previously timed out at 10 min"]
fn notgalen_closure_matches_konclude() {
    let input = Path::new("../../ontologies/external/notgalen.ofn");
    let truth = Path::new("../../ontologies/external/notgalen-classified.owx");
    if !input.exists() || !truth.exists() {
        eprintln!("SKIP: missing notgalen fixture");
        return;
    }
    let (_r, _k, fp, _m) = diff_corpus_ontology("notgalen", input, truth, 200);
    assert_eq!(fp, 0, "notgalen has FPs — soundness regression");
}

// HermiT-oracle Phase 0 fixtures (truth produced by docker/robot/classify-oracle.sh,
// which runs ROBOT's embedded HermiT). The `_hermit` suffix distinguishes these
// from the `_konclude` truth set above. The diff metric is identical: FP=0 is the
// soundness gate; MISSED is informational. See docs/phase0-corpus-candidates.md
// for selection rationale (inverse + cardinality + role-hierarchy interaction).

#[test]
#[ignore = "needs ontologies/external/ore-10908-sroiq.ofn + ore-10908-sroiq-classified.owx; ORE SROIQ (693 classes, inverse + complex roles + qualified cardinality) — Phase 0 soundness fixture"]
fn ore_10908_sroiq_closure_matches_hermit() {
    let input = Path::new("../../ontologies/external/ore-10908-sroiq.ofn");
    let truth = Path::new("../../ontologies/external/ore-10908-sroiq-classified.owx");
    if !input.exists() || !truth.exists() {
        eprintln!("SKIP: missing ore-10908-sroiq fixture");
        return;
    }
    let (_r, _k, fp, _m) = diff_corpus_ontology("ore-10908-sroiq", input, truth, test_pair_ms());
    assert_eq!(fp, 0, "ore-10908-sroiq has FPs — soundness regression");
}

#[test]
#[ignore = "needs ontologies/external/ore-15672-shoin.ofn + ore-15672-shoin-classified.owx; ORE SHOIN (83 classes, inverse + role hierarchy + unqualified cardinality) — Phase 0 soundness fixture"]
fn ore_15672_shoin_closure_matches_hermit() {
    let input = Path::new("../../ontologies/external/ore-15672-shoin.ofn");
    let truth = Path::new("../../ontologies/external/ore-15672-shoin-classified.owx");
    if !input.exists() || !truth.exists() {
        eprintln!("SKIP: missing ore-15672-shoin fixture");
        return;
    }
    let (_r, _k, fp, _m) = diff_corpus_ontology("ore-15672-shoin", input, truth, 200);
    assert_eq!(fp, 0, "ore-15672-shoin has FPs — soundness regression");
}

#[test]
#[ignore = "needs ontologies/external/shoiq-knowledge.ofn + shoiq-knowledge-classified.owx; Phase D1 fixture (was UnsupportedAxiom-erroring pre-D1; now parses via silent-drop of data axioms)"]
fn shoiq_knowledge_closure_matches_konclude() {
    let input = Path::new("../../ontologies/external/shoiq-knowledge.ofn");
    let truth = Path::new("../../ontologies/external/shoiq-knowledge-classified.owx");
    if !input.exists() || !truth.exists() {
        eprintln!("SKIP: missing shoiq-knowledge fixture");
        return;
    }
    let (_r, _k, fp, _m) = diff_corpus_ontology("shoiq-knowledge", input, truth, 200);
    assert_eq!(
        fp, 0,
        "shoiq-knowledge has FPs — D1 sound-under-approximation broken"
    );
}

#[test]
#[ignore = "needs ontologies/real/sio.ofn + konclude-input/sio-classified.owx; Phase D1 fixture (was UnsupportedAxiom-erroring pre-D1)"]
fn sio_closure_matches_konclude() {
    let input = Path::new("../../ontologies/real/sio.ofn");
    let truth = Path::new("../../ontologies/real/konclude-input/sio-classified.owx");
    if !input.exists() || !truth.exists() {
        eprintln!("SKIP: missing sio fixture");
        return;
    }
    let (_r, _k, fp, _m) = diff_corpus_ontology("sio", input, truth, test_pair_ms());
    assert_eq!(fp, 0, "sio has FPs — D1 sound-under-approximation broken");
}

#[test]
#[ignore = "needs ontologies/real/wine.ofn + konclude-input/wine-classified.owx \
            (W3C wine+food merged, circular imports stripped; HermiT oracle). \
            SHOIN(D): nominal- + disjointness-heavy expressivity stressor. \
            Fetch via scripts/fetch-real-ontologies.sh."]
fn wine_closure_matches_konclude() {
    let input = Path::new("../../ontologies/real/wine.ofn");
    let truth = Path::new("../../ontologies/real/konclude-input/wine-classified.owx");
    if !input.exists() || !truth.exists() {
        eprintln!("SKIP: missing wine fixture");
        return;
    }
    let budget = std::env::var("RUSTDL_TEST_PAIR_MS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(200);
    let (_r, _k, fp, _m) = diff_corpus_ontology("wine", input, truth, budget);
    assert_eq!(
        fp, 0,
        "wine has FPs — soundness regression on nominals/datatypes"
    );
}

#[test]
#[ignore = "needs ontologies/real/bibtex.ofn + konclude-input/bibtex-classified.owx \
            (ORE-2015 ore_ont_3341, a BibTeX ontology; HermiT oracle). \
            Datatype-heavy + real class hierarchy: 41 DataMinCardinality + 40 \
            DataPropertyDomain + 39 DataPropertyRange, 15 classes, 56 inferred \
            edges — exercises Phase-D classification on real data. Fetch via \
            scripts/fetch-real-ontologies.sh."]
fn bibtex_closure_matches_konclude() {
    let input = Path::new("../../ontologies/real/bibtex.ofn");
    let truth = Path::new("../../ontologies/real/konclude-input/bibtex-classified.owx");
    if !input.exists() || !truth.exists() {
        eprintln!("SKIP: missing bibtex fixture");
        return;
    }
    let (_r, _k, fp, _m) = diff_corpus_ontology("bibtex", input, truth, 200);
    assert_eq!(
        fp, 0,
        "bibtex has FPs — Phase-D sound-under-approximation broken"
    );
}

#[test]
#[ignore = "needs ontologies/real/ro.ofn + konclude-input/ro-classified.owx; Phase D1 fixture (was UnsupportedAxiom-erroring pre-D1; HermiT oracle generated 2026-06-03)"]
fn ro_closure_matches_konclude() {
    let input = Path::new("../../ontologies/real/ro.ofn");
    let truth = Path::new("../../ontologies/real/konclude-input/ro-classified.owx");
    if !input.exists() || !truth.exists() {
        eprintln!("SKIP: missing ro fixture");
        return;
    }
    let (_r, _k, fp, _m) = diff_corpus_ontology("ro", input, truth, 200);
    assert_eq!(fp, 0, "ro has FPs — D1 sound-under-approximation broken");
}

#[test]
#[ignore = "needs ontologies/real/sulo.ofn + konclude-input/sulo-classified.owx; Phase D1 fixture (Konclude oracle generated 2026-06-03 — ROBOT/HermiT OWX had empty <IRI/> tags that horned-owl rejects)"]
fn sulo_closure_matches_konclude() {
    let input = Path::new("../../ontologies/real/sulo.ofn");
    let truth = Path::new("../../ontologies/real/konclude-input/sulo-classified.owx");
    if !input.exists() || !truth.exists() {
        eprintln!("SKIP: missing sulo fixture");
        return;
    }
    let (_r, _k, fp, _m) = diff_corpus_ontology("sulo", input, truth, 200);
    assert_eq!(fp, 0, "sulo has FPs — D1 sound-under-approximation broken");
}

#[test]
#[ignore = "long-timeout corpus run (5 s per-pair) — measures the engine's actual completeness ceiling, independent of the standard 200 ms harness budget"]
fn corpus_closure_long_timeout() {
    let base = Path::new("../../ontologies/real");
    let cases = [
        ("pizza", "pizza.ofn", "konclude-input/pizza-classified.owx"),
        (
            "ro-stripped",
            "ro-stripped.ofn",
            "konclude-input/ro-stripped-classified.owx",
        ),
        (
            "sulo-stripped",
            "sulo-stripped.ofn",
            "konclude-input/sulo-stripped-classified.owx",
        ),
        (
            "sio-stripped",
            "sio-stripped.ofn",
            "konclude-input/sio-classified.owx",
        ),
    ];
    let mut any_fp = false;
    for (label, input, truth) in cases {
        let input_path = base.join(input);
        let truth_path = base.join(truth);
        if !input_path.exists() || !truth_path.exists() {
            eprintln!("--- {label} --- SKIP: missing fixture");
            continue;
        }
        let (_r, _k, fp, _m) = diff_corpus_ontology(label, &input_path, &truth_path, 5000);
        if fp > 0 {
            any_fp = true;
        }
    }
    assert!(
        !any_fp,
        "corpus has FPs under 5s timeout — soundness regression"
    );
}

#[test]
#[ignore = "needs ontologies/real/{pizza,ro-stripped,sulo-stripped,sio-stripped}.ofn and konclude-input/*-classified.owx"]
fn corpus_closure_matches_konclude() {
    let base = Path::new("../../ontologies/real");
    let cases = [
        ("pizza", "pizza.ofn", "konclude-input/pizza-classified.owx"),
        (
            "ro-stripped",
            "ro-stripped.ofn",
            "konclude-input/ro-stripped-classified.owx",
        ),
        (
            "sulo-stripped",
            "sulo-stripped.ofn",
            "konclude-input/sulo-stripped-classified.owx",
        ),
        (
            "sio-stripped",
            "sio-stripped.ofn",
            "konclude-input/sio-classified.owx",
        ),
    ];
    let mut any_fp = false;
    for (label, input, truth) in cases {
        let input_path = base.join(input);
        let truth_path = base.join(truth);
        if !input_path.exists() || !truth_path.exists() {
            eprintln!("--- {label} --- SKIP: missing fixture");
            continue;
        }
        let (_r, _k, fp, _m) =
            diff_corpus_ontology(label, &input_path, &truth_path, test_pair_ms());
        if fp > 0 {
            any_fp = true;
        }
    }
    assert!(!any_fp, "corpus has FPs — soundness regression");
}

#[test]
#[ignore = "Phase A1 corpus regression — family is HermiT/Konclude-inconsistent; checks rustdl's abox_check detects it (stretch: may not close without functional-merge work). Needs family.ofn."]
fn family_inconsistency_detected() {
    let path = Path::new("../../ontologies/real/family.ofn");
    if !path.exists() {
        eprintln!("SKIP: missing family.ofn");
        return;
    }
    let src = std::fs::read_to_string(path).expect("read family.ofn");
    let mut reader = Cursor::new(src);
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut reader, ParserConfiguration::default()).expect("parse");
    let consistent = owl_dl_reasoner::is_consistent(&onto).expect("is_consistent");
    eprintln!("family is_consistent = {consistent} (oracle: HermiT/Konclude inconsistent)");
    assert!(
        !consistent,
        "family should be detected as inconsistent (stretch goal)"
    );
}

#[test]
#[ignore = "Phase A1 corpus regression — family-stripped is HermiT/Konclude-inconsistent (no data axioms); checks rustdl's abox_check detects it (stretch). Needs family-stripped.ofn."]
fn family_stripped_inconsistency_detected() {
    let path = Path::new("../../ontologies/real/family-stripped.ofn");
    if !path.exists() {
        eprintln!("SKIP: missing family-stripped.ofn");
        return;
    }
    let src = std::fs::read_to_string(path).expect("read family-stripped.ofn");
    let mut reader = Cursor::new(src);
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut reader, ParserConfiguration::default()).expect("parse");
    let consistent = owl_dl_reasoner::is_consistent(&onto).expect("is_consistent");
    eprintln!(
        "family-stripped is_consistent = {consistent} (oracle: HermiT/Konclude inconsistent)"
    );
    assert!(
        !consistent,
        "family-stripped should be detected as inconsistent (stretch goal)"
    );
}

/// Map a fixture name string to `(input .ofn path, truth .owx path)`.
///
/// Paths are relative to the test binary working directory
/// (`crates/owl-dl-reasoner`), which is two levels above the repo root, so
/// all paths start with `../../`.
fn fixture_paths(fx: &str) -> (std::path::PathBuf, std::path::PathBuf) {
    match fx {
        "galen" => (
            "../../ontologies/external/galen.ofn".into(),
            "../../ontologies/external/galen-classified.owx".into(),
        ),
        "alehif" => (
            "../../ontologies/external/alehif-test.ofn".into(),
            "../../ontologies/external/alehif-test-classified.owx".into(),
        ),
        "sio" => (
            "../../ontologies/real/sio.ofn".into(),
            "../../ontologies/real/konclude-input/sio-classified.owx".into(),
        ),
        "wine" => (
            "../../ontologies/real/wine.ofn".into(),
            "../../ontologies/real/konclude-input/wine-classified.owx".into(),
        ),
        "ore-10908" => (
            "../../ontologies/external/ore-10908-sroiq.ofn".into(),
            "../../ontologies/external/ore-10908-sroiq-classified.owx".into(),
        ),
        "ore-15672" => (
            "../../ontologies/external/ore-15672-shoin.ofn".into(),
            "../../ontologies/external/ore-15672-shoin-classified.owx".into(),
        ),
        other => panic!("unknown fixture key: {other}"),
    }
}

/// Differential gate: at a generous global deadline, the global-deadline
/// classifier must produce the SAME hierarchy as the untimed classifier —
/// the deadline mechanism must not spuriously drop confirmable subsumptions.
///
/// Run:
/// ```text
/// cargo test -p owl-dl-reasoner --release --test konclude_closure_diff \
///   -- --ignored --nocapture global_deadline_differential
/// ```
#[test]
#[ignore = "long-running (classifies galen + alehif twice each); verifies the global-deadline mechanism is transparent at a generous budget"]
fn global_deadline_differential() {
    for fx in ["galen", "alehif"] {
        let (ofn, _owx) = fixture_paths(fx);
        if !ofn.exists() {
            eprintln!("SKIP {fx}: fixture missing ({})", ofn.display());
            continue;
        }
        let onto = load_ofn_fixture(&ofn);
        let untimed = owl_dl_reasoner::classify(&onto).expect("classify");
        let timed = owl_dl_reasoner::classify_with_global_deadline(
            &onto,
            std::time::Duration::from_secs(30),
        )
        .expect("classify_with_global_deadline");
        let exclude = std::collections::BTreeSet::new();
        let u = closure_from_classification(&untimed, &exclude);
        let t = closure_from_classification(&timed, &exclude);
        eprintln!(
            "{fx}: untimed_closure={} timed_closure={}",
            u.len(),
            t.len()
        );
        assert_eq!(
            t,
            u,
            "{fx}: 30 s global deadline must equal untimed hierarchy \
             (dropped={} gained={})",
            u.difference(&t).count(),
            t.difference(&u).count(),
        );
    }
}

/// Anytime per-pair sweep: for each fixture × per-pair deadline, record
/// precision / recall / silent-miss / wall vs the oracle closure. Writes a
/// CSV (env `RUSTDL_ANYTIME_CSV`, default `/tmp/anytime-per-pair.csv`).
///
/// Ignored; run explicitly:
/// ```text
/// RUSTDL_ANYTIME_CSV=docs/anytime-results-2026-06-11.csv \
/// cargo test -p owl-dl-reasoner --release --test konclude_closure_diff \
///   -- --ignored --nocapture anytime_per_pair_sweep
/// ```
///
/// Output columns: fixture, phase, deadline_ms, recall, precision,
/// silent_miss, wall_ms, undecided, true_pairs.
///
/// Soundness gate: asserts `fp == 0` at every (fixture, deadline) point.
/// A timed-out pair is counted as "undecided" (not subsumed), so it NEVER
/// appears in the rustdl closure — it is either a correctly-skipped miss
/// (flagged undecided) or a genuine NOT-subsumed (no miss). The
/// `silent_miss` column counts true-positive pairs that are neither
/// reported as subsumed NOR flagged as undecided — these are the
/// unreported, unacknowledged misses that violate the anytime contract.
#[test]
#[ignore = "long-running corpus sweep (all fixtures × 5 deadlines); run explicitly with RUSTDL_ANYTIME_CSV=<path>"]
#[allow(clippy::cast_precision_loss)] // closure sizes ≤ 50k pairs, well within f64 mantissa
fn anytime_per_pair_sweep() {
    use std::fmt::Write as _;

    let fixtures = ["galen", "alehif", "sio", "wine", "ore-10908", "ore-15672"];
    // Per-pair budgets. Capped at 100 ms: on hard SROIQ (wine, ore-15672) the
    // per-pair timeout does NOT bound total wall (each of thousands of pairs
    // may burn the full budget), so 250/1000 ms run for tens of minutes per
    // fixture while recall is already saturated by ~25-100 ms. That
    // unbounded-total-wall behaviour is precisely the motivation for the
    // Phase-2 global wall-clock deadline; it is reported qualitatively (e.g.
    // wine @ 100 ms ≈ 205 s) rather than swept to 1000 ms.
    let deadlines_ms: &[u64] = &[5, 25, 100];
    // `cargo test` runs with CWD = the package dir (crates/owl-dl-reasoner),
    // so a relative `docs/...` path would land under the package, not the
    // repo root. Resolve relative paths against the workspace root
    // (CARGO_MANIFEST_DIR/../..); absolute paths are used as-is.
    let csv_arg = std::env::var("RUSTDL_ANYTIME_CSV")
        .unwrap_or_else(|_| "/tmp/anytime-per-pair.csv".to_string());
    let csv_path = {
        let p = std::path::PathBuf::from(&csv_arg);
        if p.is_absolute() {
            p
        } else {
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../..")
                .join(&csv_arg)
        }
    };
    let mut csv = String::from(
        "fixture,phase,deadline_ms,recall,precision,silent_miss,wall_ms,undecided,true_pairs\n",
    );

    for &fx in &fixtures {
        let (input_path, truth_path) = fixture_paths(fx);
        if !input_path.exists() || !truth_path.exists() {
            eprintln!(
                "SKIP {fx}: fixture missing ({} or {})",
                input_path.display(),
                truth_path.display()
            );
            continue;
        }

        // Load oracle verdict once per fixture (deadline-independent).
        let verdict = read_konclude_verdict(&truth_path);

        // Load the ontology once per fixture (it's re-used across deadlines).
        let onto = load_ofn_fixture(&input_path);

        for &ms in deadlines_ms {
            let t0 = Instant::now();
            let h = owl_dl_reasoner::classify_with_timeout(&onto, Duration::from_millis(ms))
                .expect("classify");
            let wall_ms = t0.elapsed().as_millis();

            // Use the same symmetric exclude-set alignment as the closure-diff
            // tests — this is what makes the FP gate meaningful.
            let (reported, true_pairs) = aligned_closures(&h, &verdict);

            // Undecided pairs: timed-out probes flagged by the anytime contract.
            let undecided: BTreeSet<(String, String)> = h
                .undecided_pairs()
                .into_iter()
                .map(|(a, b)| (a.to_string(), b.to_string()))
                .collect();

            let tp = reported.intersection(&true_pairs).count();
            let fp = reported.difference(&true_pairs).count();
            let missed: BTreeSet<(String, String)> =
                true_pairs.difference(&reported).cloned().collect();
            let precision = if reported.is_empty() {
                1.0_f64
            } else {
                tp as f64 / reported.len() as f64
            };
            let recall = if true_pairs.is_empty() {
                1.0_f64
            } else {
                tp as f64 / true_pairs.len() as f64
            };
            // Silent misses: true pairs that are neither reported nor flagged.
            let silent_miss = missed.difference(&undecided).count();

            // SOUNDNESS GATE: a deadline must never produce an unsound subsumption.
            assert_eq!(fp, 0, "FP at {fx}@{ms}ms = {fp}; precision={precision:.6}");

            let _ = writeln!(
                csv,
                "{fx},per_pair,{ms},{recall:.6},{precision:.6},{silent_miss},{wall_ms},{},{}",
                undecided.len(),
                true_pairs.len()
            );
            println!(
                "{fx} @ {ms}ms: recall={recall:.4} precision={precision:.4} \
                 silent_miss={silent_miss} wall={wall_ms}ms undecided={} missed={}",
                undecided.len(),
                missed.len()
            );
        }
    }

    std::fs::write(&csv_path, csv)
        .unwrap_or_else(|e| panic!("write CSV to {}: {e}", csv_path.display()));
    println!("wrote {}", csv_path.display());
}

/// Anytime global-deadline sweep: for each fixture × global wall-clock deadline,
/// record precision / recall / silent-miss / wall vs the oracle closure. Writes /
/// appends a CSV (env `RUSTDL_ANYTIME_CSV`, default `/tmp/anytime-per-pair.csv`).
///
/// Ignored; run explicitly:
/// ```text
/// RUSTDL_ANYTIME_CSV=docs/anytime-results-2026-06-11.csv \
/// cargo test -p owl-dl-reasoner --release --test konclude_closure_diff \
///   -- --ignored --nocapture anytime_global_sweep
/// ```
///
/// Output columns: fixture, phase, deadline_ms, recall, precision,
/// silent_miss, wall_ms, undecided, true_pairs.
///
/// The `phase` column value is `"global"` (the per-pair test writes `"per_pair"`).
///
/// **Append semantics**: if `RUSTDL_ANYTIME_CSV` already exists (e.g. the
/// per-pair sweep ran first), the global rows are appended WITHOUT
/// re-writing the header.
///
/// Soundness gate: asserts `fp == 0` at every (fixture, deadline) point.
#[test]
#[ignore = "long-running corpus sweep (all fixtures × 4 global deadlines); run explicitly with RUSTDL_ANYTIME_CSV=<path>"]
#[allow(clippy::cast_precision_loss)] // closure sizes ≤ 50k pairs, well within f64 mantissa
fn anytime_global_sweep() {
    use std::fmt::Write as _;

    let fixtures = ["galen", "alehif", "sio", "wine", "ore-10908", "ore-15672"];
    // Global wall-clock deadlines — the entire classify call (all pairs) must
    // finish within this budget.
    let deadlines_ms: &[u64] = &[100, 1_000, 10_000, 30_000];
    // Resolve the CSV path the same way as the per-pair sweep.
    let csv_arg = std::env::var("RUSTDL_ANYTIME_CSV")
        .unwrap_or_else(|_| "/tmp/anytime-per-pair.csv".to_string());
    let csv_path = {
        let p = std::path::PathBuf::from(&csv_arg);
        if p.is_absolute() {
            p
        } else {
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../..")
                .join(&csv_arg)
        }
    };

    // Append semantics: keep existing contents (including header) when the
    // file already exists; otherwise seed with the header.
    let mut csv = if csv_path.exists() {
        let mut prev = std::fs::read_to_string(&csv_path).unwrap_or_default();
        // Guard against a file that doesn't end in a newline.
        if !prev.ends_with('\n') {
            prev.push('\n');
        }
        prev
    } else {
        String::from(
            "fixture,phase,deadline_ms,recall,precision,silent_miss,wall_ms,undecided,true_pairs\n",
        )
    };

    for &fx in &fixtures {
        let (input_path, truth_path) = fixture_paths(fx);
        if !input_path.exists() || !truth_path.exists() {
            eprintln!(
                "SKIP {fx}: fixture missing ({} or {})",
                input_path.display(),
                truth_path.display()
            );
            continue;
        }

        // Load oracle verdict once per fixture (deadline-independent).
        let verdict = read_konclude_verdict(&truth_path);

        // Load the ontology once per fixture (it's re-used across deadlines).
        let onto = load_ofn_fixture(&input_path);

        for &ms in deadlines_ms {
            let t0 = Instant::now();
            let h =
                owl_dl_reasoner::classify_with_global_deadline(&onto, Duration::from_millis(ms))
                    .expect("classify");
            let wall_ms = t0.elapsed().as_millis();

            // Use the same symmetric exclude-set alignment as the closure-diff
            // tests — this is what makes the FP gate meaningful.
            let (reported, true_pairs) = aligned_closures(&h, &verdict);

            // Undecided pairs: timed-out probes flagged by the anytime contract.
            let undecided: BTreeSet<(String, String)> = h
                .undecided_pairs()
                .into_iter()
                .map(|(a, b)| (a.to_string(), b.to_string()))
                .collect();

            let tp = reported.intersection(&true_pairs).count();
            let fp = reported.difference(&true_pairs).count();
            let missed: BTreeSet<(String, String)> =
                true_pairs.difference(&reported).cloned().collect();
            let precision = if reported.is_empty() {
                1.0_f64
            } else {
                tp as f64 / reported.len() as f64
            };
            let recall = if true_pairs.is_empty() {
                1.0_f64
            } else {
                tp as f64 / true_pairs.len() as f64
            };
            // Silent misses: true pairs that are neither reported nor flagged.
            let silent_miss = missed.difference(&undecided).count();

            // SOUNDNESS GATE: a deadline must never produce an unsound subsumption.
            assert_eq!(
                fp, 0,
                "FP at {fx}@{ms}ms global = {fp}; precision={precision:.6}"
            );

            let _ = writeln!(
                csv,
                "{fx},global,{ms},{recall:.6},{precision:.6},{silent_miss},{wall_ms},{},{}",
                undecided.len(),
                true_pairs.len()
            );
            println!(
                "{fx} @ {ms}ms global: recall={recall:.4} precision={precision:.4} \
                 silent_miss={silent_miss} wall={wall_ms}ms undecided={} missed={}",
                undecided.len(),
                missed.len()
            );
        }
    }

    std::fs::write(&csv_path, csv)
        .unwrap_or_else(|e| panic!("write CSV to {}: {e}", csv_path.display()));
    println!("wrote {}", csv_path.display());
}
