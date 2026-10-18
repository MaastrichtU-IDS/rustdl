//! `prove` flags a proof the cited axioms cannot complete.
//!
//! A step whose rule applies an axiom but has none recorded, or a premise with no
//! recorded derivation, used to print as if the proof were whole. The text output now
//! marks the step and ends with a `# WARNING: proof incomplete` line, and `--json`
//! carries `proof_complete`, plus `axiom_missing` and `unproved_premises` per node.

#![allow(clippy::unwrap_used)]

use std::process::Command;

fn fixture(name: &str) -> String {
    format!(
        "{}/tests/fixtures/json/{name}.ofn",
        env!("CARGO_MANIFEST_DIR")
    )
}

fn prove(args: &[&str]) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_rustdl"))
        .arg("prove")
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

fn prove_json(file: &str, sub: &str, sup: &str) -> serde_json::Value {
    serde_json::from_str(&prove(&["--json", &fixture(file), sub, sup])).unwrap()
}

/// `X ⊑ ∃R.T`, `T ⊑ ∃S.B`, `R⁻ ⊑ F`, `S ⊑ F`, `F` functional entails `X ⊑ B`. The
/// merge step rests on the three role axioms but records none of them.
const MERGE: (&str, &str, &str) = ("prove_inverse_functional_merge", "http://t/X", "http://t/B");

#[test]
fn an_uncited_step_is_flagged_in_text() {
    let (file, sub, sup) = MERGE;
    let text = prove(&[&fixture(file), sub, sup]);
    assert!(text.contains("(InverseFunctionalMerge)"), "{text}");
    assert!(text.contains("[no axiom recorded]"), "{text}");
    assert!(text.contains("# WARNING: proof incomplete"), "{text}");
}

#[test]
fn an_uncited_step_is_flagged_in_json() {
    let (file, sub, sup) = MERGE;
    let v = prove_json(file, sub, sup);
    assert_eq!(v["has_proof"], true);
    assert_eq!(v["proof_complete"], false);
    assert_eq!(v["proof"]["rule"], "InverseFunctionalMerge");
    assert_eq!(v["proof"]["axiom_missing"], true);
}

/// `X ≡ {h1, h2}`, `h1 : H`, `h2 : H` gives `X ⊑ H` as a told subsumer with no single
/// axiom behind it.
#[test]
fn a_told_subsumer_without_an_axiom_is_flagged() {
    let v = prove_json("prove_trigger_told_path_decoy", "http://t/X", "http://t/H");
    assert_eq!(v["proof_complete"], false);
}

/// Control: a proof whose steps all cite their axioms is complete and unmarked.
#[test]
fn a_complete_proof_is_not_flagged() {
    let file = "prove_conjunctive_domain";
    let v = prove_json(file, "http://ex/#X", "http://ex/#P");
    assert_eq!(v["has_proof"], true);
    assert_eq!(v["proof_complete"], true);
    let text = prove(&[&fixture(file), "http://ex/#X", "http://ex/#P"]);
    assert!(!text.contains("WARNING"), "{text}");
    assert!(!text.contains("[no axiom recorded]"), "{text}");
}

/// Control: an unrecorded `C ⊑ C` premise on a synthetic class (here the Tseitin
/// `C ⊓ D`) is reflexivity, not a gap.
#[test]
fn a_reflexive_premise_on_a_synthetic_class_is_not_flagged() {
    let v = prove_json("prove_tseitin_conjunction", "http://ex/#A", "http://ex/#E");
    assert_eq!(v["has_proof"], true);
    assert_eq!(v["proof_complete"], true);
}

/// `proof_complete` is `null` when there is no step proof (here, not entailed).
#[test]
fn no_proof_means_no_completeness_verdict() {
    let v = prove_json("prove_conjunctive_domain", "http://ex/#X", "http://ex/#B");
    assert_eq!(v["entailed"], false);
    assert_eq!(v["has_proof"], false);
    assert!(v["proof_complete"].is_null());
}

/// Control: a functional merge can mint the conjunction `A ⊓ B` at runtime, mentioned by
/// no axiom. Its projection `(A ⊓ B) ⊑ A` holds by definition and is not a gap, and the
/// class renders (it used to print as `synthetic#N` and fail `--json`).
#[test]
fn a_runtime_conjunctions_projection_is_not_flagged() {
    let file = "prove_runtime_conjunction";
    let text = prove(&[&fixture(file), "http://ex/X", "http://ex/Y"]);
    assert!(text.contains("(SyntheticDefinition)"), "{text}");
    assert!(text.contains("ObjectIntersectionOf(A B)"), "{text}");
    assert!(!text.contains("WARNING"), "{text}");
    let v = prove_json(file, "http://ex/X", "http://ex/Y");
    assert_eq!(v["proof_complete"], true);
}

/// The text warning and `proof_complete` agree on every fixture used here.
#[test]
fn text_and_json_agree_on_completeness() {
    for (file, sub, sup) in [
        MERGE,
        ("prove_trigger_told_path_decoy", "http://t/X", "http://t/H"),
        ("prove_conjunctive_domain", "http://ex/#X", "http://ex/#P"),
        ("prove_tseitin_conjunction", "http://ex/#A", "http://ex/#E"),
        ("prove_runtime_conjunction", "http://ex/X", "http://ex/Y"),
    ] {
        let warned = prove(&[&fixture(file), sub, sup]).contains("# WARNING: proof incomplete");
        let complete = prove_json(file, sub, sup)["proof_complete"] == true;
        assert_eq!(warned, !complete, "{file}: text and JSON disagree");
    }
}
