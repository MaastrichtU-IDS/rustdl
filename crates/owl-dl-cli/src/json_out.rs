//! Machine-readable JSON output for the CLI (`--json`). The stable bridge
//! contract consumed by the Protégé plugin. All arrays are sorted for
//! determinism; `schema_version` guards drift.
use std::collections::{BTreeMap, HashMap};

use horned_owl::curie::PrefixMapping;
use horned_owl::io::ofn::writer::write as write_ofn;
use horned_owl::model::{
    Build, ClassExpression, Component, Individual, MutableOntology, NamedIndividual, RcStr,
    SubClassOf,
};
use horned_owl::ontology::component_mapped::RcComponentMappedOntology;
use horned_owl::ontology::set::SetOntology;
use owl_dl_core::{ClassId, IndividualId, InternalOntology, RoleId, Vocabulary};
use owl_dl_reasoner::justify::Justification;
use owl_dl_reasoner::{
    Classification, DataPropertyValues, DifferentIndividuals, Disjointness, ObjectPropertyValues,
    PropertyClassification, ProveEntailmentResult, Realization, SameIndividuals, SyntheticDef,
};
use serde::Serialize;

/// IRI of `owl:Nothing`, used to render a `DerivedFact::Unsat` conclusion as
/// a plain `SubClassOf(C owl:Nothing)`.
const OWL_NOTHING: &str = "http://www.w3.org/2002/07/owl#Nothing";

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

/// Render a set of axioms as a self-contained OFN ontology document: a fresh
/// `SetOntology` holding exactly those axioms (no more, no fewer —
/// anti-fabrication), written with the SOURCE ontology's `PrefixMapping` so
/// prefixes round-trip on reparse. Shared by `justify --json`'s
/// per-justification rendering and `prove --json`'s per-node/fallback
/// rendering.
fn axioms_to_ofn_doc(axioms: &[Component<RcStr>], pm: &PrefixMapping) -> String {
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
            ofn: axioms_to_ofn_doc(&j.axioms, pm),
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

// ---------------------------------------------------------------------------
// `prove --json`
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub(crate) struct ProveJson {
    pub(crate) schema_version: u32,
    pub(crate) entailed: bool,
    pub(crate) has_proof: bool,
    pub(crate) proof: Option<ProofNodeJson>,
    /// OFN ontology document (present iff `has_proof` is `false` and
    /// `entailed` is `true`).
    pub(crate) justification_fallback: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct ProofNodeJson {
    /// OFN ontology document containing the single derived axiom this node
    /// proves.
    pub(crate) conclusion: String,
    /// `ElRule` name (its `Display` impl — matches the text renderer).
    pub(crate) rule: String,
    /// Source axioms used at this step, each its own OFN ontology document.
    pub(crate) axioms: Vec<String>,
    pub(crate) premises: Vec<ProofNodeJson>,
}

/// Resolve a class id to a horned-owl `ClassExpression`: a named class if
/// `id` is within the source vocabulary, else its `SyntheticDef` expansion
/// (mirrors `owl_dl_saturation::proof::render_class_expanded`/
/// `render_synthetic_def`, but building real model objects instead of a
/// display string, so the result can be written as OFN).
fn class_id_to_class_expression(
    id: ClassId,
    vocab: &Vocabulary,
    defs: &HashMap<ClassId, SyntheticDef>,
    build: &Build<RcStr>,
) -> ClassExpression<RcStr> {
    let idx = id.index() as usize;
    if idx < vocab.num_classes() {
        return ClassExpression::Class(build.class(vocab.class_iri(id)));
    }
    if let Some(def) = defs.get(&id) {
        return synthetic_def_to_class_expression(def, vocab, defs, build);
    }
    // No user-vocabulary entry and no synthetic-def record. Not expected in
    // practice (every synthetic `ClassId` the proof trace can mention has a
    // `synthetic_defs` entry — see `SyntheticDef` construction in
    // `owl-dl-saturation/src/lib.rs`), but rendered as an opaque marker class
    // rather than panicking: faithful to the internal id, not fabricated.
    ClassExpression::Class(build.class(format!("urn:rustdl-synthetic:{idx}")))
}

fn synthetic_def_to_class_expression(
    def: &SyntheticDef,
    vocab: &Vocabulary,
    defs: &HashMap<ClassId, SyntheticDef>,
    build: &Build<RcStr>,
) -> ClassExpression<RcStr> {
    match def {
        SyntheticDef::TseitinConj(bodies) => {
            let mut parts: Vec<ClassExpression<RcStr>> = bodies
                .iter()
                .map(|&b| class_id_to_class_expression(b, vocab, defs, build))
                .collect();
            if parts.len() == 1 {
                parts.remove(0)
            } else {
                ClassExpression::ObjectIntersectionOf(parts)
            }
        }
        SyntheticDef::ExistMarkerOneWay { role, body }
        | SyntheticDef::ExistMarkerEquiv { role, body } => ClassExpression::ObjectSomeValuesFrom {
            ope: role_id_to_ope(*role, vocab, build),
            bce: Box::new(class_id_to_class_expression(*body, vocab, defs, build)),
        },
        SyntheticDef::NominalKey(ind) => ClassExpression::ObjectOneOf(vec![Individual::Named(
            individual_id_to_named(*ind, vocab, build),
        )]),
        SyntheticDef::MaxKey { n, role } => ClassExpression::ObjectMaxCardinality {
            n: *n,
            ope: role_id_to_ope(*role, vocab, build),
            bce: Box::new(ClassExpression::ObjectIntersectionOf(vec![])), // unqualified: ⊤
        },
        SyntheticDef::ForallKey { role, members } => {
            let mems: Vec<Individual<RcStr>> = members
                .iter()
                .map(|&ind| Individual::Named(individual_id_to_named(ind, vocab, build)))
                .collect();
            ClassExpression::ObjectAllValuesFrom {
                ope: role_id_to_ope(*role, vocab, build),
                bce: Box::new(ClassExpression::ObjectOneOf(mems)),
            }
        }
        // Not reached in practice: `DKey` classes are interned as real named
        // vocabulary entries at conversion time (`owl_dl_core::convert`'s
        // `urn:rustdl-dkey:` classes), so they resolve via the `vocab` branch
        // above, never via `synthetic_defs`. Kept for match exhaustiveness;
        // rendered as an opaque named class if it ever fires.
        SyntheticDef::DKey(iri_suffix) => {
            ClassExpression::Class(build.class(format!("urn:rustdl-dkey:{iri_suffix}")))
        }
    }
}

fn role_id_to_ope(
    role: RoleId,
    vocab: &Vocabulary,
    build: &Build<RcStr>,
) -> horned_owl::model::ObjectPropertyExpression<RcStr> {
    horned_owl::model::ObjectPropertyExpression::ObjectProperty(
        build.object_property(vocab.role_iri(role)),
    )
}

fn individual_id_to_named(
    id: IndividualId,
    vocab: &Vocabulary,
    build: &Build<RcStr>,
) -> NamedIndividual<RcStr> {
    build.named_individual(vocab.individual_iri(id))
}

/// Render a `DerivedFact` (a saturator proof-node conclusion) as the
/// horned-owl axiom it faithfully represents — always a `SubClassOf`
/// (`Exist` is `SubClassOf(sub, ObjectSomeValuesFrom(role, target))`,
/// `Unsat` is `SubClassOf(class, owl:Nothing)`).
fn derived_fact_to_component(
    fact: &owl_dl_reasoner::DerivedFact,
    vocab: &Vocabulary,
    defs: &HashMap<ClassId, SyntheticDef>,
    build: &Build<RcStr>,
) -> Component<RcStr> {
    match fact {
        owl_dl_reasoner::DerivedFact::Sub(s, p) => Component::SubClassOf(SubClassOf {
            sub: class_id_to_class_expression(*s, vocab, defs, build),
            sup: class_id_to_class_expression(*p, vocab, defs, build),
        }),
        owl_dl_reasoner::DerivedFact::Exist(s, r, t) => Component::SubClassOf(SubClassOf {
            sub: class_id_to_class_expression(*s, vocab, defs, build),
            sup: ClassExpression::ObjectSomeValuesFrom {
                ope: role_id_to_ope(*r, vocab, build),
                bce: Box::new(class_id_to_class_expression(*t, vocab, defs, build)),
            },
        }),
        owl_dl_reasoner::DerivedFact::Unsat(c) => Component::SubClassOf(SubClassOf {
            sub: class_id_to_class_expression(*c, vocab, defs, build),
            sup: ClassExpression::Class(build.class(OWL_NOTHING)),
        }),
    }
}

/// Recursively build a `ProofNodeJson` from a `ProofNode`: the conclusion
/// and each cited source axiom are each rendered as their own self-contained
/// OFN ontology document.
fn build_proof_node_json(
    node: &owl_dl_reasoner::ProofNode,
    internal: &InternalOntology,
    defs: &HashMap<ClassId, SyntheticDef>,
    pm: &PrefixMapping,
    build: &Build<RcStr>,
) -> ProofNodeJson {
    let vocab = &internal.vocabulary;
    let conclusion_component = derived_fact_to_component(&node.conclusion, vocab, defs, build);
    let conclusion = axioms_to_ofn_doc(std::slice::from_ref(&conclusion_component), pm);

    // `node.axiom_refs` index into `internal.axioms` (the same
    // `InternalOntology` `prove_entailment_rcstr` converted the source
    // ontology into) — resolved via `owl_dl_core::axiom_to_component`, the
    // same reverse-conversion `owl_dl_core::convert_back` uses to reverse a
    // whole `InternalOntology`. An out-of-range ref is silently skipped
    // (mirrors the text renderer's `if let Some(ax) = ...` at the CLI's
    // axiom-provenance printout); it should not occur for a proof the
    // saturator itself produced.
    let axioms: Vec<String> = node
        .axiom_refs
        .iter()
        .filter_map(|r| internal.axioms.get(r.0))
        .map(|ax| {
            let component = owl_dl_core::axiom_to_component(ax, internal, build);
            axioms_to_ofn_doc(std::slice::from_ref(&component), pm)
        })
        .collect();

    let premises = node
        .premises
        .iter()
        .map(|p| build_proof_node_json(p, internal, defs, pm, build))
        .collect();

    ProofNodeJson {
        conclusion,
        rule: node.rule.to_string(),
        axioms,
        premises,
    }
}

/// Build the `prove --json` payload. `internal` must be the same ontology
/// `result` was derived from (re-converted by the caller — see
/// `prove_entailment_rcstr`'s own internal conversion).
#[must_use]
pub(crate) fn build_prove_json(
    result: &ProveEntailmentResult,
    internal: &InternalOntology,
    pm: &PrefixMapping,
) -> ProveJson {
    match result {
        ProveEntailmentResult::SaturatorProof(data) => {
            let build: Build<RcStr> = Build::new_rc();
            let proof =
                build_proof_node_json(&data.root, internal, &data.trace.synthetic_defs, pm, &build);
            ProveJson {
                schema_version: SCHEMA_VERSION,
                entailed: true,
                has_proof: true,
                proof: Some(proof),
                justification_fallback: None,
            }
        }
        ProveEntailmentResult::JustificationFallback(j) => ProveJson {
            schema_version: SCHEMA_VERSION,
            entailed: true,
            has_proof: false,
            proof: None,
            justification_fallback: Some(axioms_to_ofn_doc(&j.axioms, pm)),
        },
        ProveEntailmentResult::NotEntailed => ProveJson {
            schema_version: SCHEMA_VERSION,
            entailed: false,
            has_proof: false,
            proof: None,
            justification_fallback: None,
        },
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
