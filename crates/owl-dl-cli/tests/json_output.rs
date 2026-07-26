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

fn prophier_tiny() -> &'static str {
    concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/json/prophier_tiny.ofn"
    )
}

fn individuals_tiny() -> &'static str {
    concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/json/individuals_tiny.ofn"
    )
}

fn propvalues_tiny() -> &'static str {
    concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/json/propvalues_tiny.ofn"
    )
}

fn ce_tiny() -> &'static str {
    concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/json/ce_tiny.ofn"
    )
}

fn dropped_tiny() -> &'static str {
    concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/json/dropped_tiny.ofn"
    )
}

fn justify_el_chain() -> &'static str {
    concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/json/justify_el_chain.ofn"
    )
}

fn justify_two_paths() -> &'static str {
    concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/json/justify_two_paths.ofn"
    )
}

fn justify_sroiq() -> &'static str {
    concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/json/justify_sroiq.ofn"
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

#[test]
fn property_hierarchy_json_reports_object_edges() {
    let out = rustdl()
        .args(["property-hierarchy", "--json", prophier_tiny()])
        .output()
        .unwrap();
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).expect("stdout is valid JSON");
    assert_eq!(v["schema_version"], 1);
    assert_eq!(v["incomplete"], false);
    let obj_direct = v["object_properties"]["direct_subsumptions"]
        .as_array()
        .unwrap();
    assert!(
        obj_direct
            .iter()
            .any(|p| p[0] == "http://ex/#r" && p[1] == "http://ex/#s")
    );
    let data_direct = v["data_properties"]["direct_subsumptions"]
        .as_array()
        .unwrap();
    assert!(
        data_direct
            .iter()
            .any(|p| p[0] == "http://ex/#d1" && p[1] == "http://ex/#d2")
    );
}

#[test]
fn individuals_json_reports_different_pairs() {
    let out = rustdl()
        .args(["individuals", "--json", individuals_tiny()])
        .output()
        .unwrap();
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).expect("stdout is valid JSON");
    assert_eq!(v["schema_version"], 1);
    let different = v["different_pairs"].as_array().unwrap();
    let has = |x: &str, y: &str| {
        different.iter().any(|p| {
            let a = p.as_array().unwrap();
            (a[0] == x && a[1] == y) || (a[0] == y && a[1] == x)
        })
    };
    assert!(has("http://ex/#a", "http://ex/#b"));
}

#[test]
fn property_values_json_reports_symmetric_object_value() {
    let out = rustdl()
        .args(["property-values", "--json", propvalues_tiny()])
        .output()
        .unwrap();
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).expect("stdout is valid JSON");
    assert_eq!(v["schema_version"], 1);
    let obj = v["object_property_values"].as_array().unwrap();
    let has_triple =
        |s: &str, p: &str, o: &str| obj.iter().any(|t| t[0] == s && t[1] == p && t[2] == o);
    // Asserted: r(a,b). Symmetric ⇒ derived r(b,a) too.
    assert!(has_triple("http://ex/#a", "http://ex/#r", "http://ex/#b"));
    assert!(has_triple("http://ex/#b", "http://ex/#r", "http://ex/#a"));

    let data = v["data_property_values"].as_array().unwrap();
    assert!(data.iter().any(|t| t[0] == "http://ex/#a"
        && t[1] == "http://ex/#dp"
        && t[2] == "5"
        && t[3] == "http://www.w3.org/2001/XMLSchema#integer"));
}

#[test]
fn sat_expr_json_reports_satisfiable() {
    let out = rustdl()
        .args(["sat-expr", "--json", ce_tiny(), ":A and not :A"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).expect("stdout is valid JSON");
    assert_eq!(v["schema_version"], 1);
    assert_eq!(v["satisfiable"], false); // A ⊓ ¬A unsat
}

#[test]
fn subclass_expr_json_reports_entailed() {
    let out = rustdl()
        .args(["subclass-expr", "--json", ce_tiny(), ":A and :B", ":A"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).expect("stdout is valid JSON");
    assert_eq!(v["schema_version"], 1);
    assert_eq!(v["entailed"], true); // A⊓B ⊑ A
}

/// Parse an OFN document string and return its logical-axiom strings
/// (Manchester-rendered, prefix-free full IRIs) for containment checks.
fn ofn_doc_axiom_strings(doc: &str) -> Vec<String> {
    use horned_owl::io::ParserConfiguration;
    use horned_owl::io::ofn::reader::read as read_ofn;
    use horned_owl::model::RcStr;
    use horned_owl::ontology::set::SetOntology;
    let (onto, _): (SetOntology<RcStr>, _) = read_ofn(
        &mut std::io::Cursor::new(doc.to_owned()),
        ParserConfiguration::default(),
    )
    .expect("emitted `ofn` field must be a parseable OFN document");
    onto.iter()
        .map(|ac| format!("{:?}", ac.component))
        .collect()
}

#[test]
fn justify_json_entailed_el_chain_is_minimal() {
    let out = rustdl()
        .args([
            "justify",
            "--json",
            justify_el_chain(),
            "subclass",
            "http://ex/#A",
            "http://ex/#C",
        ])
        .output()
        .unwrap();
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).expect("stdout is valid JSON");
    assert_eq!(v["schema_version"], 1);
    assert_eq!(v["status"], "entailed");
    assert_eq!(v["minimal"], true);
    assert_eq!(v["laconic"], false);
    assert_eq!(v["enumeration_complete"], true);
    let js = v["justifications"].as_array().unwrap();
    assert_eq!(js.len(), 1, "exactly one justification for A subclass C");
    let ofn = js[0]["ofn"].as_str().unwrap();
    let axioms = ofn_doc_axiom_strings(ofn);
    assert!(
        axioms
            .iter()
            .any(|a| a.contains("http://ex/#A") && a.contains("http://ex/#B")),
        "ofn doc should contain SubClassOf(A B); got {axioms:?}"
    );
    assert!(
        axioms
            .iter()
            .any(|a| a.contains("http://ex/#B") && a.contains("http://ex/#C")),
        "ofn doc should contain SubClassOf(B C); got {axioms:?}"
    );
}

#[test]
fn justify_json_all_reports_two_justifications_complete() {
    let out = rustdl()
        .args([
            "justify",
            "--json",
            "--all",
            justify_two_paths(),
            "subclass",
            "http://ex/#A",
            "http://ex/#C",
        ])
        .output()
        .unwrap();
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).expect("stdout is valid JSON");
    assert_eq!(v["schema_version"], 1);
    assert_eq!(v["status"], "entailed");
    assert_eq!(v["enumeration_complete"], true);
    let js = v["justifications"].as_array().unwrap();
    assert_eq!(js.len(), 2, "two independent A-B-C / A-D-C paths");
}

#[test]
fn justify_json_all_max_equal_to_true_count_is_complete() {
    // justify_two_paths has exactly 2 minimal justifications (A-B-C, A-D-C).
    // `--max 2` == the true count: the max+1 probe should find no third
    // justification, so this is exhaustion, not capping.
    let out = rustdl()
        .args([
            "justify",
            "--json",
            "--all",
            "--max",
            "2",
            justify_two_paths(),
            "subclass",
            "http://ex/#A",
            "http://ex/#C",
        ])
        .output()
        .unwrap();
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).expect("stdout is valid JSON");
    assert_eq!(v["schema_version"], 1);
    assert_eq!(v["status"], "entailed");
    assert_eq!(
        v["enumeration_complete"], true,
        "max == true count must not be reported as capped"
    );
    assert_eq!(v["justifications"].as_array().unwrap().len(), 2);
}

#[test]
fn justify_json_all_max_one_below_true_count_is_capped() {
    // `--max 1` is one less than the true count of 2: the max+1 probe (2)
    // finds both, so the cap is genuine and enumeration_complete must be
    // false, with exactly `max` justifications returned.
    let out = rustdl()
        .args([
            "justify",
            "--json",
            "--all",
            "--max",
            "1",
            justify_two_paths(),
            "subclass",
            "http://ex/#A",
            "http://ex/#C",
        ])
        .output()
        .unwrap();
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).expect("stdout is valid JSON");
    assert_eq!(v["schema_version"], 1);
    assert_eq!(v["status"], "entailed");
    assert_eq!(
        v["enumeration_complete"], false,
        "max one below true count must be reported as capped"
    );
    assert_eq!(v["justifications"].as_array().unwrap().len(), 1);
}

#[test]
fn justify_json_laconic_flag_is_set() {
    let out = rustdl()
        .args([
            "justify",
            "--json",
            "--laconic",
            justify_el_chain(),
            "subclass",
            "http://ex/#A",
            "http://ex/#C",
        ])
        .output()
        .unwrap();
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).expect("stdout is valid JSON");
    assert_eq!(v["schema_version"], 1);
    assert_eq!(v["status"], "entailed");
    assert_eq!(v["laconic"], true);
    assert!(!v["justifications"].as_array().unwrap().is_empty());
}

#[test]
fn justify_json_not_entailed_reports_empty() {
    let out = rustdl()
        .args([
            "justify",
            "--json",
            justify_el_chain(),
            "subclass",
            "http://ex/#C",
            "http://ex/#A",
        ])
        .output()
        .unwrap();
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).expect("stdout is valid JSON");
    assert_eq!(v["schema_version"], 1);
    assert_eq!(v["status"], "not-entailed");
    assert!(v["justifications"].as_array().unwrap().is_empty());
}

#[test]
fn justify_json_sroiq_is_not_minimal() {
    let out = rustdl()
        .args([
            "justify",
            "--json",
            justify_sroiq(),
            "subclass",
            "http://ex/#C",
            "http://ex/#A",
        ])
        .output()
        .unwrap();
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).expect("stdout is valid JSON");
    assert_eq!(v["schema_version"], 1);
    assert_eq!(v["status"], "entailed");
    assert_eq!(v["minimal"], false);
}

#[test]
fn classify_json_reports_dropped() {
    // Fixture has a supported `SubClassOf(:A :B)` plus an unsupported
    // `HasKey(:A (:r) ())` — the confirmed "was-aborting" drop (see
    // `crates/owl-dl-reasoner/tests/dropped_axioms.rs`). Graceful
    // degradation: classify must still succeed and still report the
    // supported subsumption, with the drop surfaced in `dropped`.
    let out = rustdl()
        .args(["classify", "--json", dropped_tiny()])
        .output()
        .unwrap();
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).expect("stdout is valid JSON");
    assert_eq!(v["schema_version"], 1);

    let dropped = v["dropped"].as_object().expect("dropped is an object");
    assert!(!dropped.is_empty(), "expected a non-empty dropped block");
    assert!(
        dropped.keys().any(|k| k.contains("HasKey")),
        "expected a dropped kind mentioning HasKey, got {dropped:?}"
    );

    let direct = v["direct_subsumptions"].as_array().unwrap();
    assert!(
        direct
            .iter()
            .any(|p| p[0] == "http://ex/#A" && p[1] == "http://ex/#B"),
        "supported SubClassOf(:A :B) must still be reflected despite the dropped HasKey"
    );
}

#[test]
fn classify_json_reports_empty_dropped_when_fully_supported() {
    let out = rustdl()
        .args(["classify", "--json", tiny_consistent()])
        .output()
        .unwrap();
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).expect("stdout is valid JSON");
    let dropped = v["dropped"].as_object().expect("dropped is an object");
    assert!(
        dropped.is_empty(),
        "expected empty dropped, got {dropped:?}"
    );
}

#[test]
fn instances_expr_json_lists_instances() {
    let out = rustdl()
        .args(["instances-expr", "--json", ce_tiny(), ":A"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).expect("stdout is valid JSON");
    let insts: Vec<&str> = v["instances"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s.as_str().unwrap())
        .collect();
    assert!(insts.contains(&"http://ex/#x"));
}
