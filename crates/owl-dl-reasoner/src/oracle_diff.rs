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
