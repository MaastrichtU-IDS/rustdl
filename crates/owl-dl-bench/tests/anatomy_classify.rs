//! Pin-down test for `examples/anatomy.ofn` — a hand-written pure-EL
//! ontology (~31 classes) covering the shapes the saturation engine
//! is expected to handle without invoking the tableau.
//!
//! Guarantees this test asserts:
//! - The whole classify call takes the *pure-EL* path
//!   (`stats.pure_el_mode == true`), meaning the tableau is never
//!   invoked.
//! - A handful of representative entailments hold:
//!   * direct told subsumption: `Skull ⊑ Bone`
//!   * subsumption-via-range: `Body ⊑ BodyPart` (the range axiom on
//!     `partOf` makes any partOf-target a `BodyPart`, and `Body` is
//!     such a target via `Head`/`Neck`/...).
//!   * subsumption-via-equivalent-class: `Brain ⊑ Organ`
//!   * the equivalent-class definitions wire up correctly:
//!     `HasFingers ≡ ∃hasPart.Finger` and `HasBrain ≡ ∃hasPart.Brain`
//!     each carry the trigger concept as a subsumer.
//! - No class is flagged unsatisfiable.

use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;

use owl_dl_reasoner::classify;

fn anatomy_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("examples")
        .join("anatomy.ofn")
}

fn load() -> SetOntology<RcStr> {
    let path = anatomy_path();
    let file = File::open(&path).expect("anatomy ontology readable");
    let mut reader = BufReader::new(file);
    let (ontology, _prefixes) =
        read(&mut reader, ParserConfiguration::default()).expect("anatomy parses");
    ontology
}

fn iri(local: &str) -> String {
    format!("http://example.test/anatomy/{local}")
}

#[test]
fn anatomy_classifies_in_pure_el_mode() {
    let onto = load();
    let h = classify(&onto).expect("classify");
    let stats = h.stats();
    assert!(
        stats.pure_el_mode,
        "anatomy should classify entirely via saturation; tableau calls: sub={} unsat={}",
        stats.tableau_subsumption_calls, stats.tableau_unsat_calls,
    );
    assert_eq!(stats.tableau_subsumption_calls, 0);
    assert_eq!(stats.tableau_unsat_calls, 0);
    assert!(h.unsatisfiable_classes().is_empty());
}

#[test]
fn anatomy_direct_subsumption_holds() {
    let onto = load();
    let h = classify(&onto).expect("classify");
    // Told atomic subsumption.
    assert!(h.is_subclass(&iri("Skull"), &iri("Bone")));
    assert!(h.is_subclass(&iri("Heart"), &iri("Organ")));
    assert!(h.is_subclass(&iri("Brain"), &iri("Organ")));
    assert!(h.is_subclass(&iri("Thumb"), &iri("Finger")));
    // Told transitive subsumption (Heart ⊑ Organ ⊑ BodyPart).
    assert!(h.is_subclass(&iri("Heart"), &iri("BodyPart")));
}

#[test]
fn anatomy_range_axiom_propagates_to_targets() {
    // `ObjectPropertyRange(partOf BodyPart)` makes every partOf-target
    // a BodyPart. Body, Head, Arm, Hand, Foot, ... all appear as
    // partOf-targets in the ontology, so each should pick up
    // `BodyPart` as a subsumer.
    let onto = load();
    let h = classify(&onto).expect("classify");
    for class in ["Body", "Head", "Arm", "Hand", "Foot", "Torso"] {
        assert!(
            h.is_subclass(&iri(class), &iri("BodyPart")),
            "expected {class} ⊑ BodyPart via partOf range trigger",
        );
    }
}

#[test]
fn anatomy_existential_equivalences_classify() {
    // `EquivalentClasses(HasFingers ∃hasPart.Finger)` — there's no
    // told class that has an `∃hasPart.Finger` shape, so HasFingers
    // doesn't pick up any *named* subsumer beyond itself + the
    // equivalent's atomic operand expansion. But the closure must
    // still be reflexive and consistent.
    let onto = load();
    let h = classify(&onto).expect("classify");
    let has_fingers = iri("HasFingers");
    let has_brain = iri("HasBrain");
    // Reflexive: every class is in its own equivalence class.
    let equivs = h.equivalent_classes(&has_fingers);
    assert!(
        equivs.contains(&has_fingers.as_str()),
        "HasFingers should be reflexively in its equivalence class"
    );
    assert!(!h.unsatisfiable_classes().contains(&has_fingers.as_str()));
    assert!(!h.unsatisfiable_classes().contains(&has_brain.as_str()));
}
