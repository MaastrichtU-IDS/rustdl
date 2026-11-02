//! Closure-diff primitives for comparing a reasoner's classification against an
//! owx oracle (Konclude/HermiT/ELK output). Shared by the closure-diff tests
//! and the `owl-dl-bench matrix` harness so both use one canonical alignment.

use crate::Classification;
use horned_owl::io::ParserConfiguration;
use horned_owl::io::owx::reader::read as read_owx;
use horned_owl::model::{ClassExpression, Component, EquivalentClasses, RcStr, SubClassOf};
use horned_owl::ontology::set::SetOntology;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

const OWL_THING: &str = "http://www.w3.org/2002/07/owl#Thing";
const OWL_NOTHING: &str = "http://www.w3.org/2002/07/owl#Nothing";

/// True if `iri` denotes `owl:Thing`. ROBOT's HermiT/ELK owx output writes
/// owl:Thing as the relative IRI `<Class IRI="Thing"/>` (resolved against
/// `xml:base="…owl#"`); horned-owl resolves relative IRIs against the ontology
/// IRI, not `xml:base`, so when the classified output has no `ontologyIRI` the
/// value is left as the bare string `"Thing"`. Recognize both forms so those
/// edges are correctly excluded instead of counted as atomic subsumptions.
fn is_owl_thing(iri: &str) -> bool {
    iri == OWL_THING || iri == "Thing"
}

/// True if `iri` denotes `owl:Nothing`. See [`is_owl_thing`] for why the bare
/// relative form must also be recognized.
fn is_owl_nothing(iri: &str) -> bool {
    iri == OWL_NOTHING || iri == "Nothing"
}

/// Shorthand for the (sub, sup) IRI-string pair sets used throughout the
/// closure-diff helpers and the anytime sweep.
pub type PairSet = BTreeSet<(String, String)>;

/// Parsed owx oracle verdict: direct subsumption edges (atomic-only)
/// and the set of classes the oracle proved unsatisfiable (members of
/// some `EquivalentClasses(owl:Nothing, ...)` group).
pub struct OwxVerdict {
    /// (sub, sup) atomic-class direct edges from `SubClassOf` axioms.
    /// Excludes anything involving owl:Thing or owl:Nothing.
    pub edges: BTreeSet<(String, String)>,
    /// Members of `EquivalentClasses(owl:Nothing, ...)`. Excluded from
    /// the pair-wise comparison.
    pub unsat: BTreeSet<String>,
    /// Members of `EquivalentClasses(owl:Thing, ...)` — Thing-equivalent
    /// classes. They're trivially supersets of every other class, and
    /// every other class is a subset of them. The oracle omits these
    /// pairs from its output; rustdl correctly derives them. Treating
    /// them like owl:Thing keeps the comparison apples-to-apples.
    /// (E.g., SIO has `EquivalentClasses(owl:Thing, SIO_000000)`.)
    pub thing_equiv: BTreeSet<String>,
}

/// Read an `.owx` (OWL/XML) ontology and extract the bits we need to
/// compare against rustdl: direct atomic subsumption edges + the set
/// of unsat classes. `EquivalentClasses(X1, ..., Xn)` groups (other
/// than the unsat group ≡ owl:Nothing) are decomposed into a star of
/// bidirectional edges so they're properly included in the closure.
pub fn read_owx_verdict(path: &Path) -> anyhow::Result<OwxVerdict> {
    use anyhow::Context;
    let file = File::open(path).with_context(|| format!("open {}", path.display()))?;
    let mut reader = BufReader::new(file);
    let (onto, _): (SetOntology<RcStr>, _) = read_owx(&mut reader, ParserConfiguration::default())
        .map_err(|e| anyhow::anyhow!("parse {}: {e}", path.display()))?;
    let mut edges = BTreeSet::new();
    let mut unsat = BTreeSet::new();
    let mut thing_equiv = BTreeSet::new();
    for ax in &onto {
        match &ax.component {
            Component::SubClassOf(SubClassOf { sub, sup }) => {
                if let (ClassExpression::Class(sub_c), ClassExpression::Class(sup_c)) = (sub, sup) {
                    let s = sub_c.0.to_string();
                    let t = sup_c.0.to_string();
                    // OWLAPI's InferredSubClassAxiomGenerator (ROBOT, HermiT)
                    // writes an unsatisfiable class as `C ⊑ owl:Nothing`, not as
                    // an equivalence with it. Missing this form would turn every
                    // correct rustdl unsat class into a spurious unsat FP.
                    if is_owl_nothing(&t) && !is_owl_nothing(&s) {
                        unsat.insert(s);
                        continue;
                    }
                    if is_owl_thing(&s)
                        || is_owl_thing(&t)
                        || is_owl_nothing(&s)
                        || is_owl_nothing(&t)
                    {
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
                let has_nothing = iris.iter().any(|i| is_owl_nothing(i));
                let has_thing = iris.iter().any(|i| is_owl_thing(i));
                if has_nothing {
                    for iri in &iris {
                        if !is_owl_nothing(iri) {
                            unsat.insert(iri.clone());
                        }
                    }
                    continue;
                }
                if has_thing {
                    for iri in &iris {
                        if !is_owl_thing(iri) {
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
                        if a != b && !is_owl_thing(a) && !is_owl_thing(b) {
                            edges.insert((a.clone(), b.clone()));
                        }
                    }
                }
            }
            _ => {}
        }
    }
    Ok(OwxVerdict {
        edges,
        unsat,
        thing_equiv,
    })
}

/// Compute the transitive closure of a direct-edge set, excluding
/// reflexive pairs. The corpus closures are small enough (< 50k edges)
/// that naive Warshall over `BTreeMap` suffices.
pub fn transitive_closure(edges: &BTreeSet<(String, String)>) -> PairSet {
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
pub fn closure_from_classification(c: &Classification, exclude: &BTreeSet<String>) -> PairSet {
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

/// Given a `Classification` result and the matching `OwxVerdict`, return
/// `(rustdl_pairs, oracle_pairs)` with the same symmetric exclude-set applied
/// to both sides: `exclude = verdict.unsat ∪ rustdl_unsat ∪ verdict.thing_equiv`.
///
/// This is the single canonical alignment definition shared by the closure-diff
/// tests and the anytime sweep — keeping both callers on the same code path
/// ensures FP=0 comparisons are valid (e.g. SIO's `EquivalentClasses(owl:Thing,
/// SIO_000000)` causes spurious FPs without `thing_equiv` exclusion).
///
/// **Blind to a false unsat class on its own.** A class rustdl wrongly calls
/// unsatisfiable is in `exclude`, so it leaves both sides before the diff and
/// never counts as an FP. Every caller that gates soundness must ALSO call
/// [`unsat_diff`]. Measured: mutant M8b (`scripts/mutants/`) made 41 classes of
/// `ore_ont_10080` unsatisfiable, Konclude 0, and this pair diff read FP=0.
pub fn aligned_closures(c: &Classification, verdict: &OwxVerdict) -> (PairSet, PairSet) {
    let rustdl_unsat: BTreeSet<String> = c
        .unsatisfiable_classes()
        .iter()
        .map(ToString::to_string)
        .collect();
    let mut exclude: BTreeSet<String> = verdict.unsat.union(&rustdl_unsat).cloned().collect();
    exclude.extend(verdict.thing_equiv.iter().cloned());
    let rustdl = closure_from_classification(c, &exclude);
    let oracle_full = transitive_closure(&verdict.edges);
    let oracle: BTreeSet<(String, String)> = oracle_full
        .into_iter()
        .filter(|(s, t)| !exclude.contains(s) && !exclude.contains(t))
        .collect();
    (rustdl, oracle)
}

/// Align two owx verdicts (a reasoner's output vs the oracle's) onto the same
/// atomic-subsumption basis: exclude either side's unsat classes and either
/// side's thing-equivalent classes, then transitively close both edge sets.
/// Returns `(reasoner_pairs, oracle_pairs)`.
///
/// Blind to a false unsat class for the same reason as [`aligned_closures`];
/// pair it with [`unsat_owx_diff`].
pub fn aligned_owx_closures(reasoner: &OwxVerdict, oracle: &OwxVerdict) -> (PairSet, PairSet) {
    let mut exclude: BTreeSet<String> = reasoner.unsat.union(&oracle.unsat).cloned().collect();
    exclude.extend(reasoner.thing_equiv.iter().cloned());
    exclude.extend(oracle.thing_equiv.iter().cloned());
    let filter = |full: PairSet| -> PairSet {
        full.into_iter()
            .filter(|(s, t)| !exclude.contains(s) && !exclude.contains(t))
            .collect()
    };
    (
        filter(transitive_closure(&reasoner.edges)),
        filter(transitive_closure(&oracle.edges)),
    )
}

/// The unsatisfiable-class half of an oracle diff, which the pair diffs above
/// exclude by construction.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UnsatDiff {
    /// Classes the reasoner calls unsatisfiable and the oracle does not: a
    /// soundness failure (an unsat class is subsumed by every class).
    pub fp: BTreeSet<String>,
    /// Classes the oracle calls unsatisfiable and the reasoner does not: a
    /// completeness gap (expected under a deadline cut).
    pub missed: BTreeSet<String>,
}

/// Compare rustdl's unsatisfiable classes with the oracle's. Pair this with
/// [`aligned_closures`], which cannot see a false unsat class.
pub fn unsat_diff(c: &Classification, verdict: &OwxVerdict) -> UnsatDiff {
    let reasoner: BTreeSet<String> = c
        .unsatisfiable_classes()
        .iter()
        .map(ToString::to_string)
        .collect();
    diff_unsat(&reasoner, verdict)
}

/// [`unsat_diff`] for a reasoner whose verdict was read from an `.owx` file.
pub fn unsat_owx_diff(reasoner: &OwxVerdict, oracle: &OwxVerdict) -> UnsatDiff {
    diff_unsat(&reasoner.unsat, oracle)
}

fn diff_unsat(reasoner: &BTreeSet<String>, oracle: &OwxVerdict) -> UnsatDiff {
    let named = |i: &&String| !is_owl_thing(i) && !is_owl_nothing(i);
    // An oracle that puts owl:Thing in its unsat group says the KB is
    // inconsistent: every class is unsatisfiable, so nothing is a false unsat.
    let oracle_inconsistent = oracle.unsat.iter().any(|i| is_owl_thing(i));
    let fp = if oracle_inconsistent {
        BTreeSet::new()
    } else {
        reasoner
            .iter()
            .filter(named)
            .filter(|i| !oracle.unsat.contains(*i))
            .cloned()
            .collect()
    };
    let missed = oracle
        .unsat
        .iter()
        .filter(named)
        .filter(|i| !reasoner.contains(*i))
        .cloned()
        .collect();
    UnsatDiff { fp, missed }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// ROBOT's HermiT/ELK owx output sets `xml:base="…owl#"` on the root
    /// `<Ontology>` and writes owl:Thing/owl:Nothing as RELATIVE IRIs
    /// (`<Class IRI="Thing"/>`). horned-owl's owx reader resolves relative IRIs
    /// against the ontology IRI, not `xml:base`; when the classified output has
    /// no `ontologyIRI` attribute (the observed ROBOT case) the relative IRI is
    /// left as the bare string `"Thing"`/`"Nothing"`. This regression test
    /// guards that `read_owx_verdict` still recognizes and excludes those
    /// relative Thing/Nothing IRIs — otherwise every `X SubClassOf Thing` edge
    /// is mistaken for a real atomic subsumption, inflating the FP count by ~1
    /// per class (observed: galen FP=2748 = its class count).
    #[test]
    fn read_owx_verdict_excludes_relative_thing() {
        // Mimic ROBOT's output: xml:base on the owl# namespace, NO ontologyIRI
        // (so horned-owl leaves relative IRIs bare), one real edge, and one
        // `C SubClassOf Thing` edge written with a relative IRI.
        let doc = r#"<?xml version="1.0"?>
<Ontology xmlns="http://www.w3.org/2002/07/owl#"
     xml:base="http://www.w3.org/2002/07/owl#"
     xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"
     xmlns:xml="http://www.w3.org/XML/1998/namespace"
     xmlns:xsd="http://www.w3.org/2001/XMLSchema#"
     xmlns:rdfs="http://www.w3.org/2000/01/rdf-schema#">
    <Declaration><Class IRI="http://ex.org/C"/></Declaration>
    <Declaration><Class IRI="http://ex.org/D"/></Declaration>
    <SubClassOf><Class IRI="http://ex.org/C"/><Class IRI="http://ex.org/D"/></SubClassOf>
    <SubClassOf><Class IRI="http://ex.org/C"/><Class IRI="Thing"/></SubClassOf>
</Ontology>
"#;
        let mut path = std::env::temp_dir();
        path.push(format!(
            "rustdl-oracle-diff-relative-thing-{}.owx",
            std::process::id()
        ));
        {
            let mut f = File::create(&path).expect("create temp owx");
            f.write_all(doc.as_bytes()).expect("write temp owx");
        }
        let verdict = read_owx_verdict(&path).expect("read temp owx verdict");
        let _ = std::fs::remove_file(&path);

        // (a) the real atomic edge is present.
        assert!(
            verdict
                .edges
                .contains(&("http://ex.org/C".to_string(), "http://ex.org/D".to_string())),
            "real edge C ⊑ D must be recognized; edges = {:?}",
            verdict.edges
        );
        // (b) no edge targets a Thing IRI (relative "Thing" or absolute owl#Thing).
        assert!(
            !verdict
                .edges
                .iter()
                .any(|(_, t)| t == "Thing" || t == OWL_THING),
            "relative owl:Thing edge must be excluded; edges = {:?}",
            verdict.edges
        );
    }

    fn set(xs: &[&str]) -> BTreeSet<String> {
        xs.iter().map(ToString::to_string).collect()
    }

    fn oracle_with_unsat(unsat: &[&str]) -> OwxVerdict {
        OwxVerdict {
            edges: BTreeSet::new(),
            unsat: set(unsat),
            thing_equiv: set(&["T"]),
        }
    }

    /// The case the pair diff is blind to: rustdl says unsat, the oracle does not.
    #[test]
    fn unsat_diff_reports_a_false_unsat_class() {
        let d = diff_unsat(&set(&["A", "B"]), &oracle_with_unsat(&["A"]));
        assert_eq!(d.fp, set(&["B"]));
        assert!(d.missed.is_empty());
    }

    #[test]
    fn unsat_diff_reports_a_missed_unsat_class() {
        let d = diff_unsat(&set(&["A"]), &oracle_with_unsat(&["A", "C"]));
        assert!(d.fp.is_empty());
        assert_eq!(d.missed, set(&["C"]));
    }

    /// A thing-equivalent class is satisfiable in a consistent KB, so calling it
    /// unsat is an FP like any other.
    #[test]
    fn unsat_diff_counts_a_thing_equivalent_class() {
        let d = diff_unsat(&set(&["T"]), &oracle_with_unsat(&[]));
        assert_eq!(d.fp, set(&["T"]));
    }

    /// An inconsistent oracle KB (owl:Thing ≡ owl:Nothing, either IRI form) makes
    /// every class unsatisfiable, so no rustdl unsat class is false.
    #[test]
    fn unsat_diff_has_no_fp_against_an_inconsistent_oracle() {
        for thing in [OWL_THING, "Thing"] {
            let d = diff_unsat(&set(&["A", "Z"]), &oracle_with_unsat(&[thing, "A"]));
            assert!(d.fp.is_empty(), "{thing}: {d:?}");
        }
    }

    /// owl:Thing / owl:Nothing in either set are not classes to compare.
    #[test]
    fn unsat_diff_ignores_thing_and_nothing_names() {
        let d = diff_unsat(
            &set(&[OWL_NOTHING, "Nothing"]),
            &oracle_with_unsat(&[OWL_NOTHING]),
        );
        assert_eq!(d, UnsatDiff::default());
    }

    /// ROBOT/HermiT write an unsat class as `SubClassOf(C, owl:Nothing)`; the
    /// reader must count it as unsat in both IRI forms, or a correct rustdl
    /// unsat class reads as an unsat FP against such an oracle.
    #[test]
    fn read_owx_verdict_counts_subclass_of_nothing_as_unsat() {
        let doc = r#"<?xml version="1.0"?>
<Ontology xmlns="http://www.w3.org/2002/07/owl#"
     xml:base="http://www.w3.org/2002/07/owl#">
    <Declaration><Class IRI="http://ex.org/C"/></Declaration>
    <Declaration><Class IRI="http://ex.org/D"/></Declaration>
    <Declaration><Class IRI="http://ex.org/E"/></Declaration>
    <SubClassOf><Class IRI="http://ex.org/C"/><Class IRI="Nothing"/></SubClassOf>
    <SubClassOf><Class IRI="http://ex.org/D"/><Class IRI="http://www.w3.org/2002/07/owl#Nothing"/></SubClassOf>
    <SubClassOf><Class IRI="http://ex.org/E"/><Class IRI="http://ex.org/C"/></SubClassOf>
</Ontology>
"#;
        let mut path = std::env::temp_dir();
        path.push(format!(
            "rustdl-oracle-diff-subclass-nothing-{}.owx",
            std::process::id()
        ));
        {
            let mut f = File::create(&path).expect("create temp owx");
            f.write_all(doc.as_bytes()).expect("write temp owx");
        }
        let verdict = read_owx_verdict(&path).expect("read temp owx verdict");
        let _ = std::fs::remove_file(&path);
        assert_eq!(verdict.unsat, set(&["http://ex.org/C", "http://ex.org/D"]));
        assert!(
            verdict.edges.iter().all(|(_, t)| !t.ends_with("Nothing")),
            "no edge may target owl:Nothing: {:?}",
            verdict.edges
        );
        let d = diff_unsat(&set(&["http://ex.org/C", "http://ex.org/D"]), &verdict);
        assert!(d.fp.is_empty(), "{d:?}");
    }

    #[test]
    fn unsat_owx_diff_compares_the_two_unsat_groups() {
        let reasoner = oracle_with_unsat(&["A", "B"]);
        let d = unsat_owx_diff(&reasoner, &oracle_with_unsat(&["A", "C"]));
        assert_eq!(d.fp, set(&["B"]));
        assert_eq!(d.missed, set(&["C"]));
    }
}
