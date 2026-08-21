//! FALSE-POSITIVE CANARY for `probe_says_inconsistent`'s asserted-instance rule
//! under `DKey` id/report-position aliasing (2026-08-21).
//!
//! **The hazard.** `classify.rs`'s `unsatisfiable_idxs` is named as if it held
//! REPORT POSITIONS (indices into `Classification::classes()`), and the
//! `Classification` accessors read it that way. Its three producers do NOT:
//! each inserts `i` after deciding satisfiability of `ClassId::new(i)`, so the
//! set really holds RAW `ClassId`s clipped to `< n`. `reportable_class_iris`
//! filters `urn:rustdl-dkey:` IRIs out of the reported list, so as soon as a
//! `DKey` is interned BELOW a user class the two spaces diverge and the name is a
//! lie.
//!
//! `probe_says_inconsistent` probes that set with a raw `c.index()`, which
//! matches the producers and is therefore SOUND. Rewriting it to look the class
//! up by report position — `index[class_iri(c)]`, the change this file exists to
//! block — compares a position against a set of raw ids and turns a CONSISTENT
//! KB into a global inconsistency verdict, under which every pair is vacuously
//! entailed. That is the maximal false positive, at default settings
//! (`RUSTDL_CLASSIFY_CONSISTENCY_PROBE` is default-ON).
//!
//! **The mutation that kills this test** (verified, not asserted): in
//! `probe_says_inconsistent`, replace
//!
//! ```text
//! && unsatisfiable_idxs.contains(&(c.index() as usize))
//! ```
//!
//! with a report-position lookup
//!
//! ```text
//! && let Some(&pos) = index.get(internal.vocabulary.class_iri(*c))
//! && unsatisfiable_idxs.contains(&pos)
//! ```
//!
//! (threading `index: &HashMap<String, usize>` in from both call sites). Under
//! that edit [`dkey_aliased_abox_assertion_is_not_an_inconsistency`] reports
//! `inconsistent = true` with all four classes unsatisfiable.
//!
//! **This test is fix-agnostic.** It asserts only the OBSERVABLE — the KB is
//! consistent and not every class is unsatisfiable — cross-checked against
//! `is_consistent` on the same input. Any genuine repair of the aliasing (e.g.
//! a `ReportedClasses` type fixing producers and consumers together) keeps it
//! green; only a one-ended "fix" of the probe breaks it. It deliberately does
//! NOT pin `unsatisfiable_classes()`, which is separately wrong here (it names
//! `:N` rather than `:M`) and is the aliasing bug proper.
//!
//! Run: `cargo test -p owl-dl-reasoner --test classify_dkey_alias_consistency`

#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_core::ClassId;
use std::io::Cursor;

const PFX: &str = "Prefix(:=<http://ex.org/>)\n\
                   Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)\n\
                   Prefix(owl:=<http://www.w3.org/2002/07/owl#>)\n";

/// `xsd:integer[0, 5]` — lowers to the synthetic class `urn:rustdl-dkey:0:5`.
const R05: &str = "DatatypeRestriction(xsd:integer xsd:minInclusive \"0\"^^xsd:integer \
                   xsd:maxInclusive \"5\"^^xsd:integer)";

/// Every ingredient the probe's asserted-instance rule needs, arranged so the
/// aliasing lines up exactly:
///
/// * `:A`, `:B` are DECLARED, so they intern first (`DeclareClass` sorts before
///   every axiom) and take ids 0, 1.
/// * `SubClassOf(DataSomeValuesFrom(:p [0,5]) :A)` is the first AXIOM to sort,
///   so the `DKey` interns at id **2** — below the user classes that follow.
/// * `:M` and `:N` are USED BUT UNDECLARED, so they intern after the `DKey`, at
///   ids **3** and **4**. Their report positions are 2 and 3 (the `DKey` is
///   filtered out), so `report_position(:N) == raw_id(:M) == 3` — the collision.
/// * `:M` is unsatisfiable (`:M ⊑ :B` and `:M ⊑ ¬:B`) but has no instances.
/// * `:N` is satisfiable and is the subject of an `ABox` `ClassAssertion`.
/// * The union and max-cardinality axioms push the ontology out of the EL
///   fragment, so classify takes the hybrid path that actually calls
///   `probe_says_inconsistent` (the pure-EL path never does).
///
/// The KB is CONSISTENT: `:M` being empty is not an inconsistency.
fn source() -> String {
    format!(
        "{PFX}Ontology(<http://ex.org/dkey-alias>
Declaration(Class(:A))
Declaration(Class(:B))
Declaration(DataProperty(:p))
Declaration(ObjectProperty(:r))
Declaration(NamedIndividual(:x))
SubClassOf(DataSomeValuesFrom(:p {R05}) :A)
SubClassOf(:M :B)
SubClassOf(:N :B)
SubClassOf(:M ObjectComplementOf(:B))
SubClassOf(:A ObjectMaxCardinality(1 :r))
SubClassOf(:B ObjectUnionOf(:A :N))
ClassAssertion(:N :x)
)
"
    )
}

fn parse() -> SetOntology<RcStr> {
    let mut reader = Cursor::new(source());
    let (onto, _) = read_ofn(&mut reader, ParserConfiguration::default()).expect("parse ofn");
    onto
}

/// The fixture only exercises the hazard if the `DKey` really does sit below a
/// reported class AND the asserted class's report position really does collide
/// with another class's raw id. Both are properties of the interning order,
/// which a change to component sorting or to the data lowering could silently
/// dissolve — leaving the canary green while proving nothing. Assert them.
#[test]
fn fixture_actually_aliases() {
    let internal = owl_dl_core::convert::convert_ontology(&parse()).expect("convert");
    let iri = |k: u32| internal.vocabulary.class_iri(ClassId::new(k)).to_owned();

    let ids: Vec<String> = (0..internal.vocabulary.num_classes())
        .map(|k| iri(u32::try_from(k).unwrap()))
        .collect();
    assert_eq!(
        ids,
        vec![
            "http://ex.org/A".to_owned(),
            "http://ex.org/B".to_owned(),
            "urn:rustdl-dkey:0:5".to_owned(),
            "http://ex.org/M".to_owned(),
            "http://ex.org/N".to_owned(),
        ],
        "interning order drifted; the DKey must sit at id 2, BELOW :M and :N, or \
         report positions and ClassIds no longer diverge and this file tests nothing"
    );

    let classify = owl_dl_reasoner::classify(&parse()).expect("classify");
    let classes = classify.classes();
    let pos = |name: &str| {
        classes
            .iter()
            .position(|c| c == name)
            .unwrap_or_else(|| panic!("{name} must be reported"))
    };
    assert!(
        !classes
            .iter()
            .any(|c| c.starts_with(owl_dl_core::DKEY_IRI_PREFIX)),
        "DKeys must stay out of the reported class list — that filtering is the \
         whole reason the two index spaces diverge"
    );
    assert_eq!(
        pos("http://ex.org/N"),
        3,
        "the collision under test: :N's REPORT POSITION must equal :M's RAW ClassId (3)"
    );
    assert_eq!(pos("http://ex.org/M"), 2, ":M's report position must be 2");
}

/// The observable. `:M` is unsatisfiable and `:N` — a satisfiable class whose
/// report position collides with `:M`'s raw id — has an `ABox` instance. That is
/// the exact firing shape for `probe_says_inconsistent`'s asserted-instance
/// rule, and the answer must still be CONSISTENT.
#[test]
fn dkey_aliased_abox_assertion_is_not_an_inconsistency() {
    // Ground truth from the sibling surface, on the same input.
    assert!(
        owl_dl_reasoner::is_consistent(&parse()).expect("is_consistent"),
        "fixture must be consistent — :M is empty, which is not an inconsistency"
    );

    let classify = owl_dl_reasoner::classify(&parse()).expect("classify");

    // Non-vacuity: the probe's first gate is `unsatisfiable_idxs.is_empty()`.
    // With an empty set it returns before ever reaching the asserted-instance
    // rule and this test could not fail however that rule is written.
    assert!(
        !classify.unsatisfiable_classes().is_empty(),
        "the unsat set must be non-empty or the probe short-circuits and this \
         canary is vacuous"
    );

    assert!(
        !classify.stats().inconsistent,
        "FALSE POSITIVE: classify declared a consistent KB inconsistent. Under an \
         inconsistency verdict every pair is vacuously entailed, so this is the \
         maximal wrong answer. See this file's header: the asserted-instance rule \
         in `probe_says_inconsistent` must probe `unsatisfiable_idxs` with a raw \
         `ClassId`, because that is what its producers store."
    );
    assert_ne!(
        classify.unsatisfiable_classes().len(),
        classify.classes().len(),
        "all-classes-unsatisfiable is the `classify_inconsistent` signature — the \
         same false positive, seen from the class list"
    );
    assert!(
        !classify.is_subclass("http://ex.org/A", "http://ex.org/N"),
        "vacuous entailment leaking out of an inconsistency verdict"
    );
}
