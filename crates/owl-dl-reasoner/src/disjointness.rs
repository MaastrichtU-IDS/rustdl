//! Inferred disjointness queries (issue #47). Sound: a disjoint pair is
//! reported only when `C ⊓ D` is proven unsatisfiable (or told disjoint).
use crate::{PreparedOntology, ReasonError};
use horned_owl::model::ForIRI;
use horned_owl::ontology::set::SetOntology;
use owl_dl_core::convert::convert_ontology;
use std::time::{Duration, Instant};

/// Entailed disjoint named-class pairs, plus a completeness flag.
#[derive(Debug, Clone)]
pub struct Disjointness {
    pairs: Vec<(String, String)>,
    incomplete: bool,
}

impl Disjointness {
    /// `(c, d)` pairs with `c < d`, sorted and deduplicated, over named
    /// satisfiable classes (excludes owl:Thing/owl:Nothing, unsatisfiable
    /// classes, and self-pairs).
    #[must_use]
    pub fn pairs(&self) -> &[(String, String)] {
        &self.pairs
    }

    /// `true` iff a probe timed out, or classification was not
    /// complete-by-construction — i.e. the reported set may be missing
    /// entailed pairs.
    #[must_use]
    pub fn incomplete(&self) -> bool {
        self.incomplete
    }
}

const THING: &str = "http://www.w3.org/2002/07/owl#Thing";
const NOTHING: &str = "http://www.w3.org/2002/07/owl#Nothing";

/// Entailed disjoint named-class pairs. `pair_deadline` bounds each `C ⊓ D`
/// probe, AND each per-class/per-pair probe the up-front classification pass
/// runs to find the unsatisfiable-class set; `None` = unbounded.
///
/// # Errors
/// [`ReasonError::Inconsistent`] if the ontology is inconsistent;
/// [`ReasonError::Conversion`] if the input can't be lowered to the
/// internal IR.
pub fn disjoint_classes<A: ForIRI>(
    onto: &SetOntology<A>,
    pair_deadline: Option<Duration>,
) -> Result<Disjointness, ReasonError> {
    let internal = convert_ontology(onto)?;
    if crate::abox_saturation::saturate_abox_consistency(&internal).clash {
        return Err(ReasonError::Inconsistent);
    }
    // Candidate named classes = declared classes minus unsat/Thing/Nothing.
    // Use classify to get the unsat set + the reportable class list. Threads
    // the same `pair_deadline` the caller gave us into the per-class/per-pair
    // probes classification itself runs (`classify_internal` == `_with_timeout`
    // with `None`, so this is a no-op when the caller passes `None`) — without
    // this, classification's own probes ignored `pair_deadline` entirely and
    // could dominate the wall time this function is trying to bound.
    let classification = crate::classify::classify_internal_with_timeout(&internal, pair_deadline)?;
    let unsat: std::collections::HashSet<&str> =
        classification.unsatisfiable_classes().into_iter().collect();
    let mut incomplete = !classification.completeness_guaranteed();
    // `from_internal` clones `internal.vocabulary` before consuming `internal`,
    // so `prepared.vocabulary` resolves the same IRI ↔ id mapping.
    let prepared = PreparedOntology::from_internal(internal)?;
    let mut names: Vec<(String, owl_dl_core::ir::ClassId)> = Vec::new();
    for c in classification.classes() {
        if c == THING || c == NOTHING || unsat.contains(c.as_str()) {
            continue;
        }
        if let Some(id) = prepared.vocabulary.class_id(c) {
            names.push((c.clone(), id));
        }
    }
    let mut pairs: Vec<(String, String)> = Vec::new();
    for i in 0..names.len() {
        for j in (i + 1)..names.len() {
            let (ci, cj) = (names[i].1, names[j].1);
            // Told-disjoint (asserted `DisjointClasses`/`Not`-subclass/
            // `DisjointUnion`) is sound on its own — the pair is entailed
            // disjoint without running the tableau probe at all. Task 1.1:
            // this is what turns the O(n^2) full-probe loop into "probe only
            // the pairs that aren't already known".
            if prepared.told.are_told_disjoint(ci, cj) {
                let (a, b) = (&names[i].0, &names[j].0);
                let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
                pairs.push((lo.clone(), hi.clone()));
                continue;
            }
            let deadline = pair_deadline.map(|d| Instant::now() + d);
            match prepared.pair_disjoint_with_deadline(ci, cj, deadline)? {
                Some(true) => {
                    let (a, b) = (&names[i].0, &names[j].0);
                    let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
                    pairs.push((lo.clone(), hi.clone()));
                }
                Some(false) => {}
                None => incomplete = true,
            }
        }
    }
    pairs.sort();
    pairs.dedup();
    Ok(Disjointness { pairs, incomplete })
}

fn pairwise_sorted(names: &[String]) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for i in 0..names.len() {
        for j in (i + 1)..names.len() {
            let (a, b) = (&names[i], &names[j]);
            let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
            out.push((lo.clone(), hi.clone()));
        }
    }
    out
}

/// Told-disjoint object property pairs (`a < b`), read directly from the
/// horned-owl ontology's `DisjointObjectProperties` axioms. Structural only
/// (no entailment probe): only named `ObjectProperty` members are kept,
/// `InverseObjectProperty` members are skipped.
///
/// # Errors
/// [`ReasonError::Inconsistent`] if the ontology is inconsistent;
/// [`ReasonError::Conversion`] if the input can't be lowered to the internal
/// IR.
pub fn disjoint_object_properties<A: ForIRI>(
    onto: &SetOntology<A>,
) -> Result<Vec<(String, String)>, ReasonError> {
    use horned_owl::model::Component as C;
    use horned_owl::model::ObjectPropertyExpression as OPE;

    let internal = convert_ontology(onto)?;
    if crate::abox_saturation::saturate_abox_consistency(&internal).clash {
        return Err(ReasonError::Inconsistent);
    }
    let mut out: Vec<(String, String)> = Vec::new();
    for ac in onto {
        if let C::DisjointObjectProperties(ax) = &ac.component {
            let names: Vec<String> =
                ax.0.iter()
                    .filter_map(|ope| match ope {
                        OPE::ObjectProperty(op) => Some(op.0.as_ref().to_string()),
                        OPE::InverseObjectProperty(_) => None,
                    })
                    .collect();
            out.extend(pairwise_sorted(&names));
        }
    }
    out.sort();
    out.dedup();
    Ok(out)
}

/// Told-disjoint data property pairs (`a < b`), read directly from the
/// horned-owl ontology's `DisjointDataProperties` axioms. Structural only
/// (no entailment probe).
///
/// # Errors
/// [`ReasonError::Inconsistent`] if the ontology is inconsistent;
/// [`ReasonError::Conversion`] if the input can't be lowered to the internal
/// IR.
pub fn disjoint_data_properties<A: ForIRI>(
    onto: &SetOntology<A>,
) -> Result<Vec<(String, String)>, ReasonError> {
    use horned_owl::model::Component as C;

    let internal = convert_ontology(onto)?;
    if crate::abox_saturation::saturate_abox_consistency(&internal).clash {
        return Err(ReasonError::Inconsistent);
    }
    let mut out: Vec<(String, String)> = Vec::new();
    for ac in onto {
        if let C::DisjointDataProperties(ax) = &ac.component {
            let names: Vec<String> = ax.0.iter().map(|dp| dp.0.as_ref().to_string()).collect();
            out.extend(pairwise_sorted(&names));
        }
    }
    out.sort();
    out.dedup();
    Ok(out)
}
