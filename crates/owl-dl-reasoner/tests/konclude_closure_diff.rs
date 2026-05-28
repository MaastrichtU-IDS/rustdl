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
    let (onto, _): (SetOntology<RcStr>, _) =
        read_owx(&mut reader, ParserConfiguration::default())
            .unwrap_or_else(|e| panic!("parse {}: {e}", path.display()));
    let mut edges = BTreeSet::new();
    let mut unsat = BTreeSet::new();
    let mut thing_equiv = BTreeSet::new();
    for ax in onto.iter() {
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

/// Print and return (rustdl_closure, konclude_closure, fp, missed).
fn diff_corpus_ontology(
    label: &str,
    input: &Path,
    truth: &Path,
    per_pair_ms: u64,
) -> (usize, usize, usize, usize) {
    let src = std::fs::read_to_string(input).expect("read input");
    let mut reader = Cursor::new(src);
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut reader, ParserConfiguration::default()).expect("parse input");
    let start = Instant::now();
    let c =
        classify_top_down_with_timeout(&onto, Duration::from_millis(per_pair_ms)).expect("classify");
    let wall = start.elapsed();
    let verdict = read_konclude_verdict(truth);
    // Build the full exclude set up-front (unsat from both sides +
    // Thing-equivalent classes Konclude omits by convention), then
    // apply it symmetrically to both closures.
    let rustdl_unsat: BTreeSet<String> = c
        .unsatisfiable_classes()
        .iter()
        .map(ToString::to_string)
        .collect();
    let mut exclude: BTreeSet<String> = verdict.unsat.union(&rustdl_unsat).cloned().collect();
    exclude.extend(verdict.thing_equiv.iter().cloned());
    let rustdl = closure_from_classification(&c, &exclude);
    let konclude_full = transitive_closure(&verdict.edges);
    let konclude: BTreeSet<(String, String)> = konclude_full
        .into_iter()
        .filter(|(s, t)| !exclude.contains(s) && !exclude.contains(t))
        .collect();
    let fp: BTreeSet<_> = rustdl.difference(&konclude).cloned().collect();
    let missed: BTreeSet<_> = konclude.difference(&rustdl).cloned().collect();
    eprintln!(
        "--- {label} ({:.2} s) ---\nrustdl_closure={} konclude_closure={} FP={} MISSED={} (unsat: rustdl={} konclude={} thing-equiv: {})",
        wall.as_secs_f64(),
        rustdl.len(),
        konclude.len(),
        fp.len(),
        missed.len(),
        rustdl_unsat.len(),
        verdict.unsat.len(),
        verdict.thing_equiv.len(),
    );
    for (s, t) in fp.iter().take(5) {
        eprintln!(" FP: {s} ⊑ {t}");
    }
    for (s, t) in missed.iter().take(5) {
        eprintln!(" MISSED: {s} ⊑ {t}");
    }
    (rustdl.len(), konclude.len(), fp.len(), missed.len())
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
        let (_r, _k, fp, _m) = diff_corpus_ontology(label, &input_path, &truth_path, 200);
        if fp > 0 {
            any_fp = true;
        }
    }
    assert!(!any_fp, "corpus has FPs — soundness regression");
}
