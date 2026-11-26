//! #594: `prove --json --source-fallback` adds a source-only justification next to a step
//! proof that a source-only consumer (the Protégé plugin) cannot show: one whose steps use
//! an axiom rustdl added during conversion (`builtin_axioms` / `derived_axioms`, which
//! `proof_uses_non_source_axioms` reports), or that is not complete. Without the flag, or
//! for a proof that cites source axioms only, nothing is added.

#![allow(clippy::unwrap_used)]

use std::collections::BTreeSet;
use std::io::Cursor;
use std::process::Command;

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read;
use horned_owl::model::{Component, RcStr};
use horned_owl::ontology::set::SetOntology;

const NOTHING: &str = "http://www.w3.org/2002/07/owl#Nothing";

fn fixture(name: &str) -> String {
    format!(
        "{}/tests/fixtures/prove_cites_source/{name}.ofn",
        env!("CARGO_MANIFEST_DIR")
    )
}

fn components(doc: &str) -> BTreeSet<Component<RcStr>> {
    let (o, _): (SetOntology<RcStr>, _) = read(
        &mut Cursor::new(doc.to_owned()),
        ParserConfiguration::default(),
    )
    .unwrap();
    o.iter()
        .map(|ac| ac.component.clone())
        .filter(|c| !matches!(c, Component::OntologyID(_) | Component::DocIRI(_)))
        .collect()
}

fn prove(file: &str, sub: &str, sup: &str, fallback: bool) -> serde_json::Value {
    let path = fixture(file);
    let mut args = vec!["prove", "--json"];
    if fallback {
        args.push("--source-fallback");
    }
    args.extend(["--", &path, sub, sup]);
    let out = Command::new(env!("CARGO_BIN_EXE_rustdl"))
        .env_remove("RUSTDL_HOME")
        .env_remove("RUSTDL_ORE_DIR")
        .args(&args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{file}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}

/// The fallback's axioms, each asserted to be a component of the source.
fn fallback_axioms(file: &str, v: &serde_json::Value) -> BTreeSet<Component<RcStr>> {
    let doc = v["justification_fallback"]
        .as_str()
        .unwrap_or_else(|| panic!("{file}: no fallback: {v}"));
    let source = components(&std::fs::read_to_string(fixture(file)).unwrap());
    let cited = components(doc);
    assert!(!cited.is_empty(), "{file}: an empty fallback: {v}");
    for c in &cited {
        assert!(
            source.contains(c),
            "{file}: fallback axiom not in the source: {c:?}"
        );
    }
    cited
}

#[test]
fn a_proof_using_a_derived_axiom_gets_a_source_fallback() {
    let v = prove("dkey_edge", "http://t/X", "http://t/D", true);
    assert_eq!(v["has_proof"], true, "{v}");
    assert_eq!(v["proof_uses_non_source_axioms"], true, "{v}");
    assert_eq!(fallback_axioms("dkey_edge", &v).len(), 2, "{v}");
    assert!(v["justification_minimal"].is_boolean(), "{v}");
}

#[test]
fn an_unsat_goal_goes_through_the_same_path() {
    let v = prove("unsat_dkey", "http://t/X", NOTHING, true);
    assert_eq!(v["proof_uses_non_source_axioms"], true, "{v}");
    let cited = fallback_axioms("unsat_dkey", &v);
    assert!(
        cited
            .iter()
            .any(|c| matches!(c, Component::DisjointClasses(_))),
        "{cited:?}"
    );
}

#[test]
fn nothing_is_added_without_the_flag_or_for_a_source_only_proof() {
    let v = prove("dkey_edge", "http://t/X", "http://t/D", false);
    assert_eq!(v["proof_uses_non_source_axioms"], true, "{v}");
    assert!(v["justification_fallback"].is_null(), "{v}");
    let v = prove("chain3", "http://t/X", "http://t/D", true);
    assert_eq!(v["has_proof"], true, "{v}");
    assert_eq!(v["proof_uses_non_source_axioms"], false, "{v}");
    assert!(v["justification_fallback"].is_null(), "{v}");
}

#[test]
fn source_fallback_needs_json() {
    let out = Command::new(env!("CARGO_BIN_EXE_rustdl"))
        .args([
            "prove",
            "--source-fallback",
            &fixture("chain3"),
            "http://t/X",
            "http://t/D",
        ])
        .output()
        .unwrap();
    assert!(!out.status.success());
}
