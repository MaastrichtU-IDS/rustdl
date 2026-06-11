//! Pin-down test for `examples/family.ofn` — a small ontology that
//! intentionally lives outside the saturation fragment (uses
//! `ObjectUnionOf` and `InverseObjectProperties`). Asserts the
//! orchestrator drops to hybrid mode and the tableau confirms the
//! expected entailments.

use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;

use owl_dl_reasoner::classify;

fn family_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("examples")
        .join("family.ofn")
}

fn load() -> SetOntology<RcStr> {
    let path = family_path();
    let file = File::open(&path).expect("family ontology readable");
    let mut reader = BufReader::new(file);
    let (ontology, _prefixes) =
        read(&mut reader, ParserConfiguration::default()).expect("family parses");
    ontology
}

fn iri(local: &str) -> String {
    format!("http://example.test/family/{local}")
}

#[test]
fn family_classifies_in_hybrid_mode() {
    let onto = load();
    let h = classify(&onto).expect("classify");
    let stats = h.stats();
    assert!(
        !stats.pure_el_mode,
        "family uses union + inverse roles, should be hybrid"
    );
    // The non-EL fragment is exercised beyond told-subsumption, confirmed by
    // a representative entailment that needs it. We deliberately do NOT
    // assert `tableau_subsumption_calls + tableau_unsat_calls > 0`: that
    // counter tracks MAIN-tableau calls only, and the default-ON hypertableau
    // wedge + per-class label heuristic now resolve family's entailments
    // within the hybrid path without a main-tableau call (it drops to 0 under
    // `RUSTDL_LABEL_HEURISTIC` default-on — same optimization-drift class as
    // the reasoner's flag-pinned stale tests). The wedge IS part of the
    // hybrid engine, so a 0 main-tableau count is correct, not a regression.
    assert!(
        h.is_subclass(&iri("Man"), &iri("Person")),
        "hybrid reasoning must still confirm the non-EL entailment Man ⊑ Person",
    );
}

#[test]
fn family_gendered_persons_subsume_person() {
    let onto = load();
    let h = classify(&onto).expect("classify");
    assert!(h.is_subclass(&iri("Man"), &iri("Person")));
    assert!(h.is_subclass(&iri("Woman"), &iri("Person")));
}

#[test]
fn family_mother_subsumed_by_woman_and_parent() {
    let onto = load();
    let h = classify(&onto).expect("classify");
    assert!(h.is_subclass(&iri("Mother"), &iri("Woman")));
    assert!(h.is_subclass(&iri("Mother"), &iri("Parent")));
    assert!(h.is_subclass(&iri("Father"), &iri("Man")));
}

#[test]
fn family_male_disjoint_from_female() {
    // Disjointness + the union axiom doesn't force Male ⊑ Female to
    // be false in the classify matrix — it forces the *intersection*
    // to be empty. The classifier should report neither Male ⊑ Female
    // nor the reverse.
    let onto = load();
    let h = classify(&onto).expect("classify");
    assert!(!h.is_subclass(&iri("Male"), &iri("Female")));
    assert!(!h.is_subclass(&iri("Female"), &iri("Male")));
}
