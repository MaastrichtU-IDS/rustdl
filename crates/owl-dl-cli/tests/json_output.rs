//! End-to-end test for the `--json` output mode (schema v1).

#![allow(clippy::unwrap_used)]

use std::process::Command;

fn rustdl() -> Command {
    Command::new(env!("CARGO_BIN_EXE_rustdl"))
}

fn tiny_consistent() -> &'static str {
    concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/json/consistent_tiny.ofn"
    )
}

fn tiny_inconsistent() -> &'static str {
    concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/json/inconsistent_tiny.ofn"
    )
}

fn tiny_abox() -> &'static str {
    concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/json/abox_tiny.ofn"
    )
}

fn equivalent_pair() -> &'static str {
    concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/json/equivalent_pair.ofn"
    )
}

fn unsat_pair() -> &'static str {
    concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/json/unsat_pair.ofn"
    )
}

fn disjoint_tiny() -> &'static str {
    concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/json/disjoint_tiny.ofn"
    )
}

#[test]
fn classify_json_parses_and_reports_consistent() {
    let out = rustdl()
        .args(["classify", "--json", tiny_consistent()])
        .output()
        .unwrap();
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).expect("stdout is valid JSON");
    assert_eq!(v["schema_version"], 1);
    assert_eq!(v["consistent"], true);
    assert!(v["direct_subsumptions"].is_array());
}

#[test]
fn consistent_json_reports_inconsistent() {
    let out = rustdl()
        .args(["consistent", "--json", tiny_inconsistent()])
        .output()
        .unwrap();
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).expect("stdout is valid JSON");
    assert_eq!(v["schema_version"], 1);
    assert_eq!(v["consistent"], false);
}

#[test]
fn realize_json_reports_types() {
    let out = rustdl()
        .args(["realize", "--json", tiny_abox()])
        .output()
        .unwrap();
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).expect("stdout is valid JSON");
    assert_eq!(v["schema_version"], 1);
    let inds = v["individuals"].as_array().unwrap();
    let x = inds
        .iter()
        .find(|i| i["iri"] == "http://ex/#x")
        .expect("x realized");
    let types: Vec<&str> = x["types"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t.as_str().unwrap())
        .collect();
    assert!(types.contains(&"http://ex/#A"));
    assert!(types.contains(&"http://ex/#B"));
}

#[test]
fn classify_json_reports_equivalent_group() {
    let out = rustdl()
        .args(["classify", "--json", equivalent_pair()])
        .output()
        .unwrap();
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).expect("stdout is valid JSON");
    assert_eq!(v["schema_version"], 1);
    assert_eq!(v["consistent"], true);
    let groups = v["equivalent_groups"].as_array().unwrap();
    let group = groups
        .iter()
        .find(|g| g.as_array().unwrap().iter().any(|m| m == "http://ex/#A"))
        .expect("a group containing A");
    let members: Vec<&str> = group
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m.as_str().unwrap())
        .collect();
    // In the fixture, :A is deliberately UNDECLARED: `convert_ontology` sorts
    // components before interning, and `DeclareClass` sorts first (by IRI), so
    // declared classes always get byte-ordered ids. Undeclared :A is interned
    // later via the EquivalentClasses axiom, so the pre-sort group (ascending
    // class-id order from `Classification::equivalent_classes`) is [B, A];
    // only the byte-order `group.sort()` in build_classify_json yields [A, B].
    // Exact equality therefore makes that sort load-bearing.
    assert_eq!(
        members,
        vec!["http://ex/#A", "http://ex/#B"],
        "group members are byte-sorted"
    );
}

#[test]
fn classify_json_excludes_unsat_from_equivalent_groups() {
    // :C and :D are both unsatisfiable (each ⊑ :A ⊓ :B with :A, :B disjoint).
    // Every unsatisfiable class is mutually equivalent (≡ ⊥), so without the
    // exclusion the whole unsat set would surface as a spurious equivalence
    // group. Unsat classes belong in `unsatisfiable` (the bottom node), not in
    // `equivalent_groups`.
    let out = rustdl()
        .args(["classify", "--json", unsat_pair()])
        .output()
        .unwrap();
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).expect("stdout is valid JSON");
    assert_eq!(v["schema_version"], 1);
    let unsat: Vec<&str> = v["unsatisfiable"]
        .as_array()
        .unwrap()
        .iter()
        .map(|u| u.as_str().unwrap())
        .collect();
    assert!(unsat.contains(&"http://ex/#C"), "C is unsatisfiable");
    assert!(unsat.contains(&"http://ex/#D"), "D is unsatisfiable");
    for group in v["equivalent_groups"].as_array().unwrap() {
        for member in group.as_array().unwrap() {
            let iri = member.as_str().unwrap();
            assert!(
                !unsat.contains(&iri),
                "equivalent_groups must not contain unsatisfiable class {iri}"
            );
        }
    }
}

#[test]
fn disjoint_json_reports_class_pairs() {
    let out = rustdl()
        .args(["disjoint", "--json", disjoint_tiny()])
        .output()
        .unwrap();
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).expect("stdout is valid JSON");
    assert_eq!(v["schema_version"], 1);
    let dc = v["disjoint_classes"].as_array().unwrap();
    let has = |x: &str, y: &str| {
        dc.iter().any(|p| {
            let a = p.as_array().unwrap();
            (a[0] == x && a[1] == y) || (a[0] == y && a[1] == x)
        })
    };
    assert!(has("http://ex/#A", "http://ex/#B"));
}
