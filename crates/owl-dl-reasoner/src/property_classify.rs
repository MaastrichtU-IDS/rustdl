//! Inferred property hierarchy (issue #44). Structural closure — complete for
//! the fragment the reasoner reasons about (told + equivalent + inverse for
//! object properties, told + equivalent for data). No entailment probe.
use crate::ReasonError;
use horned_owl::model::ForIRI;
use horned_owl::ontology::set::SetOntology;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone)]
pub struct PropertyClassification {
    equivalent_groups: Vec<Vec<String>>,
    direct_subsumptions: Vec<(String, String)>,
}
impl PropertyClassification {
    #[must_use]
    pub fn equivalent_groups(&self) -> &[Vec<String>] {
        &self.equivalent_groups
    }
    #[must_use]
    pub fn direct_subsumptions(&self) -> &[(String, String)] {
        &self.direct_subsumptions
    }
}

/// Turn a transitive `(sub, sup)` closure into equivalence groups + Hasse edges.
fn from_closure(closure: Vec<(String, String)>) -> PropertyClassification {
    let sub_of: BTreeSet<(String, String)> = closure.into_iter().collect();
    // Equivalence: a≡b iff (a,b) and (b,a) both present.
    let mut nodes: BTreeSet<String> = BTreeSet::new();
    for (a, b) in &sub_of {
        nodes.insert(a.clone());
        nodes.insert(b.clone());
    }
    // Group by mutual reachability.
    let mut group_of: BTreeMap<String, usize> = BTreeMap::new();
    let mut groups: Vec<Vec<String>> = Vec::new();
    for n in &nodes {
        if group_of.contains_key(n) {
            continue;
        }
        let mut grp = vec![n.clone()];
        for m in &nodes {
            if m != n
                && sub_of.contains(&(n.clone(), m.clone()))
                && sub_of.contains(&(m.clone(), n.clone()))
            {
                grp.push(m.clone());
            }
        }
        grp.sort();
        grp.dedup();
        let idx = groups.len();
        for g in &grp {
            group_of.insert(g.clone(), idx);
        }
        groups.push(grp);
    }
    // Representative = lexicographically smallest member.
    let rep = |g: usize| groups[g][0].clone();
    // Direct subsumption between DISTINCT groups: rep_a ⊑ rep_b with no rep_c strictly between.
    let mut strict: BTreeSet<(usize, usize)> = BTreeSet::new();
    for (a, b) in &sub_of {
        let (ga, gb) = (group_of[a], group_of[b]);
        if ga != gb {
            strict.insert((ga, gb));
        }
    }
    let mut direct: Vec<(String, String)> = Vec::new();
    for &(ga, gb) in &strict {
        let redundant = strict
            .iter()
            .any(|&(gx, gy)| gx == ga && gy != gb && strict.contains(&(gy, gb)));
        if !redundant {
            direct.push((rep(ga), rep(gb)));
        }
    }
    direct.sort();
    direct.dedup();
    let mut equivalent_groups: Vec<Vec<String>> =
        groups.into_iter().filter(|g| g.len() > 1).collect();
    equivalent_groups.sort();
    PropertyClassification {
        equivalent_groups,
        direct_subsumptions: direct,
    }
}

/// # Errors
/// [`ReasonError::Inconsistent`] / [`ReasonError::Conversion`].
pub fn classify_object_property_hierarchy<A: ForIRI>(
    onto: &SetOntology<A>,
) -> Result<PropertyClassification, ReasonError> {
    Ok(from_closure(crate::materialize_subobjectproperty_axioms(
        onto,
    )?))
}

/// # Errors
/// [`ReasonError::Inconsistent`] / [`ReasonError::Conversion`].
pub fn classify_data_property_hierarchy<A: ForIRI>(
    onto: &SetOntology<A>,
) -> Result<PropertyClassification, ReasonError> {
    Ok(from_closure(crate::materialize_subdataproperty_axioms(
        onto,
    )?))
}
