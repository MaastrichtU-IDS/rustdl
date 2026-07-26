//! Machine-readable JSON output for the CLI (`--json`). The stable bridge
//! contract consumed by the Protégé plugin. All arrays are sorted for
//! determinism; `schema_version` guards drift.
use std::collections::BTreeMap;

use horned_owl::curie::PrefixMapping;
use horned_owl::io::ofn::writer::write as write_ofn;
use horned_owl::model::{Component, MutableOntology, RcStr};
use horned_owl::ontology::component_mapped::RcComponentMappedOntology;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::justify::Justification;
use owl_dl_reasoner::{
    Classification, DataPropertyValues, DifferentIndividuals, Disjointness, ObjectPropertyValues,
    PropertyClassification, Realization, SameIndividuals,
};
use serde::Serialize;

const SCHEMA_VERSION: u32 = 1;

/// The per-kind dropped-axiom counts for `ontology`, as a `--json` block.
///
/// NOTE: this re-runs `convert_ontology` (via `dropped_axioms`), which the
/// caller has typically already done once to reason over the ontology — one
/// extra conversion per CLI invocation, negligible next to actual reasoning.
/// Kept as a separate call (rather than threading it out of the existing
/// `Classification`/`Realization` results) because those types don't carry
/// the conversion's `DroppedAxioms` through their public API.
#[must_use]
pub(crate) fn dropped_block(onto: &SetOntology<RcStr>) -> BTreeMap<String, u64> {
    owl_dl_reasoner::dropped_axioms(onto)
        .map(|d| d.by_kind().clone())
        .unwrap_or_default()
}

#[derive(Serialize)]
pub(crate) struct ClassifyJson {
    pub(crate) schema_version: u32,
    pub(crate) consistent: bool,
    pub(crate) incomplete: bool,
    pub(crate) unsatisfiable: Vec<String>,
    pub(crate) equivalent_groups: Vec<Vec<String>>,
    pub(crate) direct_subsumptions: Vec<[String; 2]>,
    pub(crate) dropped: BTreeMap<String, u64>,
}

#[derive(Serialize)]
pub(crate) struct ConsistentJson {
    pub(crate) schema_version: u32,
    pub(crate) consistent: bool,
    pub(crate) dropped: BTreeMap<String, u64>,
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
    pub(crate) dropped: BTreeMap<String, u64>,
}

#[derive(Serialize)]
pub(crate) struct DisjointJson {
    pub(crate) schema_version: u32,
    pub(crate) incomplete: bool,
    pub(crate) disjoint_classes: Vec<[String; 2]>,
    pub(crate) disjoint_object_properties: Vec<[String; 2]>,
    pub(crate) disjoint_data_properties: Vec<[String; 2]>,
}

#[derive(Serialize)]
pub(crate) struct SatExprJson {
    pub(crate) schema_version: u32,
    pub(crate) incomplete: bool,
    pub(crate) satisfiable: bool,
}

#[derive(Serialize)]
pub(crate) struct SubclassExprJson {
    pub(crate) schema_version: u32,
    pub(crate) incomplete: bool,
    pub(crate) entailed: bool,
}

#[derive(Serialize)]
pub(crate) struct InstancesExprJson {
    pub(crate) schema_version: u32,
    pub(crate) incomplete: bool,
    pub(crate) instances: Vec<String>,
}

#[must_use]
pub(crate) fn build_sat_expr_json(v: owl_dl_reasoner::CeVerdict) -> SatExprJson {
    SatExprJson {
        schema_version: SCHEMA_VERSION,
        incomplete: v.incomplete(),
        satisfiable: v.holds(),
    }
}

#[must_use]
pub(crate) fn build_subclass_expr_json(v: owl_dl_reasoner::CeVerdict) -> SubclassExprJson {
    SubclassExprJson {
        schema_version: SCHEMA_VERSION,
        incomplete: v.incomplete(),
        entailed: v.holds(),
    }
}

#[must_use]
pub(crate) fn build_instances_expr_json(r: &owl_dl_reasoner::CeInstances) -> InstancesExprJson {
    let mut instances = r.individuals().to_vec();
    instances.sort();
    InstancesExprJson {
        schema_version: SCHEMA_VERSION,
        incomplete: r.incomplete(),
        instances,
    }
}

#[must_use]
pub(crate) fn build_classify_json(
    h: &Classification,
    dropped: BTreeMap<String, u64>,
) -> ClassifyJson {
    let stats = h.stats();

    let mut unsatisfiable: Vec<String> = h
        .unsatisfiable_classes()
        .into_iter()
        .map(str::to_owned)
        .collect();
    unsatisfiable.sort();

    // Equivalence groups: for each class, its equivalence peers; canonicalise
    // by sorting each group and deduping groups (a group is emitted once).
    // Unsatisfiable classes are EXCLUDED: they are all mutually equivalent
    // (≡ ⊥), so `equivalent_classes` returns the whole unsat set for any of
    // them — a spurious peer group. They belong in `unsatisfiable` (the
    // bottom node), not in `equivalent_groups`.
    let unsat_set: std::collections::HashSet<&str> =
        unsatisfiable.iter().map(String::as_str).collect();
    let mut groups: Vec<Vec<String>> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for c in h.classes() {
        if seen.contains(c) || unsat_set.contains(c.as_str()) {
            continue;
        }
        let mut group: Vec<String> = h
            .equivalent_classes(c)
            .into_iter()
            .filter(|e| !unsat_set.contains(e))
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
        dropped,
    }
}

#[must_use]
pub(crate) fn build_consistent_json(
    consistent: bool,
    dropped: BTreeMap<String, u64>,
) -> ConsistentJson {
    ConsistentJson {
        schema_version: SCHEMA_VERSION,
        consistent,
        dropped,
    }
}

#[must_use]
pub(crate) fn build_realize_json(r: &Realization, dropped: BTreeMap<String, u64>) -> RealizeJson {
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
        dropped,
    }
}

#[derive(Serialize)]
pub(crate) struct PropHierSide {
    pub(crate) equivalent_groups: Vec<Vec<String>>,
    pub(crate) direct_subsumptions: Vec<[String; 2]>,
}

#[derive(Serialize)]
pub(crate) struct PropHierJson {
    pub(crate) schema_version: u32,
    pub(crate) incomplete: bool,
    pub(crate) object_properties: PropHierSide,
    pub(crate) data_properties: PropHierSide,
}

fn side(c: &PropertyClassification) -> PropHierSide {
    let mut ds: Vec<[String; 2]> = c
        .direct_subsumptions()
        .iter()
        .map(|(a, b)| [a.clone(), b.clone()])
        .collect();
    ds.sort();
    let mut eg: Vec<Vec<String>> = c.equivalent_groups().to_vec();
    eg.sort();
    PropHierSide {
        equivalent_groups: eg,
        direct_subsumptions: ds,
    }
}

#[must_use]
pub(crate) fn build_prophier_json(
    obj: &PropertyClassification,
    data: &PropertyClassification,
) -> PropHierJson {
    PropHierJson {
        schema_version: SCHEMA_VERSION,
        incomplete: false,
        object_properties: side(obj),
        data_properties: side(data),
    }
}

#[must_use]
pub(crate) fn build_disjoint_json(
    classes: &Disjointness,
    obj: Vec<(String, String)>,
    data: Vec<(String, String)>,
) -> DisjointJson {
    let to_arr = |v: Vec<(String, String)>| {
        let mut a: Vec<[String; 2]> = v.into_iter().map(|(x, y)| [x, y]).collect();
        a.sort();
        a
    };
    let mut dc: Vec<[String; 2]> = classes
        .pairs()
        .iter()
        .map(|(x, y)| [x.clone(), y.clone()])
        .collect();
    dc.sort();
    DisjointJson {
        schema_version: SCHEMA_VERSION,
        incomplete: classes.incomplete(),
        disjoint_classes: dc,
        disjoint_object_properties: to_arr(obj),
        disjoint_data_properties: to_arr(data),
    }
}

#[derive(Serialize)]
pub(crate) struct IndividualsJson {
    pub(crate) schema_version: u32,
    pub(crate) incomplete: bool,
    pub(crate) same_groups: Vec<Vec<String>>,
    pub(crate) different_pairs: Vec<[String; 2]>,
}

#[must_use]
pub(crate) fn build_individuals_json(
    same: &SameIndividuals,
    different: &DifferentIndividuals,
) -> IndividualsJson {
    let mut same_groups: Vec<Vec<String>> = same
        .groups()
        .iter()
        .map(|g| {
            let mut g = g.clone();
            g.sort();
            g
        })
        .collect();
    same_groups.sort();

    let mut different_pairs: Vec<[String; 2]> = different
        .pairs()
        .iter()
        .map(|(a, b)| {
            let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
            [lo.clone(), hi.clone()]
        })
        .collect();
    different_pairs.sort();

    IndividualsJson {
        schema_version: SCHEMA_VERSION,
        incomplete: same.incomplete() || different.incomplete(),
        same_groups,
        different_pairs,
    }
}

#[derive(Serialize)]
pub(crate) struct PropertyValuesJson {
    pub(crate) schema_version: u32,
    pub(crate) incomplete: bool,
    pub(crate) object_property_values: Vec<[String; 3]>,
    pub(crate) data_property_values: Vec<[String; 4]>,
}

#[must_use]
pub(crate) fn build_property_values_json(
    obj: &ObjectPropertyValues,
    data: &DataPropertyValues,
) -> PropertyValuesJson {
    let mut object_property_values: Vec<[String; 3]> = obj
        .triples()
        .iter()
        .map(|(s, p, o)| [s.clone(), p.clone(), o.clone()])
        .collect();
    object_property_values.sort();

    let mut data_property_values: Vec<[String; 4]> = data
        .quads()
        .iter()
        .map(|(s, p, lex, dt)| [s.clone(), p.clone(), lex.clone(), dt.clone()])
        .collect();
    data_property_values.sort();

    PropertyValuesJson {
        schema_version: SCHEMA_VERSION,
        incomplete: obj.incomplete() || data.incomplete(),
        object_property_values,
        data_property_values,
    }
}

#[derive(Serialize)]
pub(crate) struct JustifyJson {
    pub(crate) schema_version: u32,
    pub(crate) status: String, // "entailed" | "not-entailed"
    pub(crate) enumeration_complete: bool,
    pub(crate) minimal: bool, // all justifications minimal_guaranteed
    pub(crate) laconic: bool,
    pub(crate) justifications: Vec<JustificationJson>,
}

#[derive(Serialize)]
pub(crate) struct JustificationJson {
    pub(crate) ofn: String, // self-contained OFN ontology document
}

/// Render one justification's axioms as a self-contained OFN ontology
/// document: a fresh `SetOntology` holding exactly those axioms (no more, no
/// fewer — anti-fabrication), written with the SOURCE ontology's
/// `PrefixMapping` so prefixes round-trip on reparse.
fn justification_ofn_doc(axioms: &[Component<RcStr>], pm: &PrefixMapping) -> String {
    let mut so: SetOntology<RcStr> = SetOntology::new();
    for ax in axioms {
        so.insert(ax.clone());
    }
    let cmo: RcComponentMappedOntology = so.into();
    let mut buf: Vec<u8> = Vec::new();
    write_ofn(&mut buf, &cmo, Some(pm)).expect(
        "writing a justification's axioms (no OntologyID, single ontology) to an in-memory \
         buffer cannot fail",
    );
    String::from_utf8(buf).expect("horned-owl's OFN writer emits valid UTF-8")
}

/// Build the `justify --json` payload. `enumeration_complete` reflects
/// whether `justs` is the FULL set of minimal justifications (always `true`
/// for the default single-justification query; for `--all`, `false` when the
/// `--max` cap was hit — see the call site).
#[must_use]
pub(crate) fn build_justify_json(
    justs: &[Justification<RcStr>],
    pm: &PrefixMapping,
    laconic: bool,
    enumeration_complete: bool,
) -> JustifyJson {
    let minimal = justs.iter().all(|j| j.minimal_guaranteed);
    let justifications = justs
        .iter()
        .map(|j| JustificationJson {
            ofn: justification_ofn_doc(&j.axioms, pm),
        })
        .collect();
    JustifyJson {
        schema_version: SCHEMA_VERSION,
        status: if justs.is_empty() {
            "not-entailed"
        } else {
            "entailed"
        }
        .to_owned(),
        enumeration_complete,
        minimal,
        laconic,
        justifications,
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
        let j = build_classify_json(&h, BTreeMap::new());
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
