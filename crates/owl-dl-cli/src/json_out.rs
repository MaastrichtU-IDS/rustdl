//! Machine-readable JSON output for the CLI (`--json`). The stable bridge
//! contract consumed by the Protégé plugin. All arrays are sorted for
//! determinism; `schema_version` guards drift.
// TODO(Task 2): remove this allow once the CLI `--json` flag wires these in.
#![allow(dead_code)]
use owl_dl_reasoner::{Classification, Realization};
use serde::Serialize;

const SCHEMA_VERSION: u32 = 1;

#[derive(Serialize)]
pub(crate) struct ClassifyJson {
    pub(crate) schema_version: u32,
    pub(crate) consistent: bool,
    pub(crate) incomplete: bool,
    pub(crate) unsatisfiable: Vec<String>,
    pub(crate) equivalent_groups: Vec<Vec<String>>,
    pub(crate) direct_subsumptions: Vec<[String; 2]>,
}

#[derive(Serialize)]
pub(crate) struct ConsistentJson {
    pub(crate) schema_version: u32,
    pub(crate) consistent: bool,
}

#[derive(Serialize)]
pub(crate) struct IndividualTypesJson {
    pub(crate) iri: String,
    pub(crate) types: Vec<String>,
    pub(crate) direct_types: Vec<String>,
}

#[derive(Serialize)]
pub(crate) struct RealizeJson {
    pub(crate) schema_version: u32,
    pub(crate) individuals: Vec<IndividualTypesJson>,
}

#[must_use]
pub(crate) fn build_classify_json(h: &Classification) -> ClassifyJson {
    let stats = h.stats();

    let mut unsatisfiable: Vec<String> = h
        .unsatisfiable_classes()
        .into_iter()
        .map(str::to_owned)
        .collect();
    unsatisfiable.sort();

    // Equivalence groups: for each class, its equivalence peers; canonicalise
    // by sorting each group and deduping groups (a group is emitted once).
    let mut groups: Vec<Vec<String>> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for c in h.classes() {
        if seen.contains(c) {
            continue;
        }
        let mut group: Vec<String> = h
            .equivalent_classes(c)
            .into_iter()
            .map(str::to_owned)
            .collect();
        if !group.iter().any(|g| g == c) {
            group.push(c.clone());
        }
        group.sort();
        group.dedup();
        for g in &group {
            seen.insert(g.clone());
        }
        if group.len() > 1 {
            groups.push(group);
        }
    }
    groups.sort();

    let mut direct_subsumptions: Vec<[String; 2]> = Vec::new();
    for c in h.classes() {
        for sup in h.direct_subsumers(c) {
            direct_subsumptions.push([c.clone(), sup.to_owned()]);
        }
    }
    direct_subsumptions.sort();
    direct_subsumptions.dedup();

    ClassifyJson {
        schema_version: SCHEMA_VERSION,
        consistent: !stats.inconsistent,
        incomplete: stats.timed_out_pairs > 0,
        unsatisfiable,
        equivalent_groups: groups,
        direct_subsumptions,
    }
}

#[must_use]
pub(crate) fn build_consistent_json(consistent: bool) -> ConsistentJson {
    ConsistentJson {
        schema_version: SCHEMA_VERSION,
        consistent,
    }
}

#[must_use]
pub(crate) fn build_realize_json(r: &Realization) -> RealizeJson {
    let mut individuals: Vec<IndividualTypesJson> = r
        .individuals()
        .iter()
        .map(|ind| {
            let mut types: Vec<String> = r.entailed_types(ind).to_vec();
            types.sort();
            let mut direct_types: Vec<String> = r.most_specific_types(ind).to_vec();
            direct_types.sort();
            IndividualTypesJson {
                iri: ind.clone(),
                types,
                direct_types,
            }
        })
        .collect();
    individuals.sort_by(|a, b| a.iri.cmp(&b.iri));
    RealizeJson {
        schema_version: SCHEMA_VERSION,
        individuals,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use horned_owl::io::ParserConfiguration;
    use horned_owl::io::ofn::reader::read as read_ofn;
    use horned_owl::model::RcStr;
    use horned_owl::ontology::set::SetOntology;
    use std::io::Cursor;

    fn classify_ofn(src: &str) -> owl_dl_reasoner::Classification {
        let (onto, _): (SetOntology<RcStr>, _) = read_ofn(
            &mut Cursor::new(src.to_owned()),
            ParserConfiguration::default(),
        )
        .unwrap();
        owl_dl_reasoner::classify(&onto).unwrap()
    }

    #[test]
    fn classify_json_is_sorted_and_carries_verdict() {
        // B ⊑ A, C ⊑ B ⇒ direct B⊑A, C⊑B; consistent; nothing unsat.
        let h = classify_ofn(
            r"Prefix(:=<http://ex/#>)
              Ontology(<http://ex/>
                Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C))
                SubClassOf(:B :A) SubClassOf(:C :B))",
        );
        let j = build_classify_json(&h);
        assert_eq!(j.schema_version, 1);
        assert!(j.consistent);
        assert!(!j.incomplete);
        assert!(j.unsatisfiable.is_empty());
        // direct edges present, sorted by (sub, sup):
        assert!(
            j.direct_subsumptions
                .contains(&["http://ex/#B".to_owned(), "http://ex/#A".to_owned()])
        );
        assert!(
            j.direct_subsumptions
                .contains(&["http://ex/#C".to_owned(), "http://ex/#B".to_owned()])
        );
        // sorted invariant:
        let mut sorted = j.direct_subsumptions.clone();
        sorted.sort();
        assert_eq!(j.direct_subsumptions, sorted);
    }
}
