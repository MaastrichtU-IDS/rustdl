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
/// probe; `None` = unbounded.
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
    // Use classify to get the unsat set + the reportable class list.
    let classification = crate::classify_internal(&internal)?;
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
            let deadline = pair_deadline.map(|d| Instant::now() + d);
            match prepared.pair_disjoint_with_deadline(names[i].1, names[j].1, deadline)? {
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
