//! #538 on the CLI: repeated operands of n-ary axioms are read as a set. The JSON
//! surfaces report `collapsed_operands`; `consistent` no longer says inconsistent for
//! `DifferentIndividuals(:a :a)`; the disjoint-property surface's told arm
//! (`RUSTDL_PROPERTY_DISJOINT_DOWN=0`) no longer reports `(r, r)`; and with
//! `RUSTDL_DATA_PROPERTIES=0` a `DisjointDataProperties` is still only dropped. The env
//! arms are process-global, hence a subprocess each.
#![allow(clippy::unwrap_used)]

use std::path::PathBuf;
use std::process::Command;

/// Writes the fixture to a per-test file (tests run in parallel).
struct Fixture(PathBuf);

impl Fixture {
    fn new(tag: &str, body: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "rustdl-dup-operands-{}-{tag}.ofn",
            std::process::id()
        ));
        let src = format!(
            "Prefix(:=<http://ex/#>)\nPrefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)\n\
             Ontology(<http://ex/o>\n\
             Declaration(Class(:C)) Declaration(Class(:D))\n\
             Declaration(ObjectProperty(:r)) Declaration(ObjectProperty(:s))\n\
             Declaration(DataProperty(:p)) Declaration(DataProperty(:q))\n\
             Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))\n\
             {body}\n)\n"
        );
        std::fs::write(&path, src).unwrap();
        Self(path)
    }

    fn path(&self) -> &str {
        self.0.to_str().unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// (exit code, stdout, stderr).
fn run(env: &[(&str, &str)], args: &[&str]) -> (i32, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_rustdl"))
        .envs(env.iter().copied())
        .args(args)
        .output()
        .unwrap();
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8(out.stdout).unwrap(),
        String::from_utf8(out.stderr).unwrap(),
    )
}

fn json(s: &str) -> serde_json::Value {
    serde_json::from_str(s).unwrap_or_else(|e| panic!("{e}: {s}"))
}

#[test]
fn consistent_reads_repeated_individuals_as_a_set() {
    let f = Fixture::new(
        "diff",
        "DifferentIndividuals(:a :a :b) ClassAssertion(:C :a)",
    );
    let (code, out, err) = run(&[], &["consistent", "--json", f.path()]);
    assert_eq!(code, 0, "{err}");
    let v = json(&out);
    assert_eq!(v["consistent"], true);
    assert_eq!(v["collapsed_operands"]["axioms"], 1);
    assert_eq!(v["collapsed_operands"]["vacuous"], 0);
    assert!(err.contains("repeated an operand"), "{err}");
    // Control: the distinct pair still clashes with an equality.
    let f = Fixture::new(
        "diff-ctl",
        "DifferentIndividuals(:a :a :b) SameIndividual(:a :b)",
    );
    let (_, out, _) = run(&[], &["consistent", "--json", f.path()]);
    assert_eq!(json(&out)["consistent"], false);
}

#[test]
fn classify_json_reports_collapsed_operands() {
    let f = Fixture::new("classify", "DisjointClasses(:C :C) SubClassOf(:D :C)");
    let (code, out, err) = run(&[], &["classify", "--json", f.path()]);
    assert_eq!(code, 0, "{err}");
    let v = json(&out);
    assert_eq!(v["unsatisfiable"], serde_json::json!([]));
    assert_eq!(
        v["collapsed_operands"],
        serde_json::json!({"axioms": 1, "vacuous": 1})
    );
    assert_eq!(v["dropped"], serde_json::json!({}));
    let f = Fixture::new("classify-none", "DisjointClasses(:C :D)");
    let (_, out, err) = run(&[], &["classify", "--json", f.path()]);
    assert_eq!(
        json(&out)["collapsed_operands"],
        serde_json::json!({"axioms": 0, "vacuous": 0})
    );
    assert!(!err.contains("repeated an operand"), "{err}");
}

#[test]
fn both_disjoint_property_arms_read_repeats_as_a_set() {
    let f = Fixture::new(
        "dop",
        "DisjointObjectProperties(:r :r) SubObjectPropertyOf(:s :r) \
         DisjointDataProperties(:p :p) SubDataPropertyOf(:q :p)",
    );
    let ctl = Fixture::new(
        "dop-ctl",
        "DisjointObjectProperties(:r :s) DisjointDataProperties(:p :q)",
    );
    for down in ["1", "0"] {
        let env = [("RUSTDL_PROPERTY_DISJOINT_DOWN", down)];
        let (code, out, err) = run(&env, &["disjoint", "--json", f.path()]);
        assert_eq!(code, 0, "{err}");
        let v = json(&out);
        assert_eq!(
            v["disjoint_object_properties"],
            serde_json::json!([]),
            "down={down}"
        );
        assert_eq!(
            v["disjoint_data_properties"],
            serde_json::json!([]),
            "down={down}"
        );
        let (_, out, _) = run(&env, &["disjoint", "--json", ctl.path()]);
        let v = json(&out);
        assert_eq!(
            v["disjoint_object_properties"],
            serde_json::json!([["http://ex/#r", "http://ex/#s"]]),
            "control, down={down}"
        );
        assert_eq!(
            v["disjoint_data_properties"],
            serde_json::json!([["http://ex/#p", "http://ex/#q"]]),
            "control, down={down}"
        );
    }
}

#[test]
fn data_gate_off_still_only_drops_disjoint_data_properties() {
    let f = Fixture::new(
        "ddp",
        "DisjointDataProperties(:p :p) DataPropertyAssertion(:p :a \"1\"^^xsd:integer)",
    );
    for gate in ["1", "0"] {
        let (code, out, err) = run(
            &[("RUSTDL_DATA_PROPERTIES", gate)],
            &["consistent", "--json", f.path()],
        );
        assert_eq!(code, 0, "{err}");
        let v = json(&out);
        assert_eq!(v["consistent"], true, "gate={gate}");
        let dropped = v["dropped"].as_object().unwrap();
        let ddp = dropped
            .keys()
            .any(|k| k.starts_with("DisjointDataProperties"));
        assert_eq!(ddp, gate == "0", "{v}");
    }
}
