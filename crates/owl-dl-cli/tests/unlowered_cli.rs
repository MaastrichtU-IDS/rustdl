//! `rustdl unlowered` (hidden diagnostic, layer 2 of the pure-EL guard): one JSON object
//! naming what the EL lowering drops under the fast-path certification. The off-CI corpus
//! sweep runs it per file, so its shape is pinned here.

#![allow(clippy::unwrap_used)]

use std::path::PathBuf;
use std::process::Command;

struct Fixture(PathBuf);

impl Fixture {
    fn new(tag: &str, body: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "rustdl-unlowered-cli-{}-{tag}.ofn",
            std::process::id()
        ));
        let src = format!(
            "Prefix(:=<http://ex/#>)\nPrefix(owl:=<http://www.w3.org/2002/07/owl#>)\n\
             Ontology(<http://ex/o>\n\
             Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:X)) \
             Declaration(ObjectProperty(:r))\n{body}\n)\n"
        );
        std::fs::write(&path, src).unwrap();
        Self(path)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn unlowered(f: &Fixture) -> serde_json::Value {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_rustdl"));
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("RUSTDL_") {
            cmd.env_remove(key);
        }
    }
    let out = cmd.arg("unlowered").arg(&f.0).output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}

/// The known live drop (issue draft `top-lhs-existential`) is reported, with its site and
/// the dropped construct.
#[test]
fn reports_the_known_top_lhs_drop() {
    let f = Fixture::new(
        "top",
        "SubClassOf(owl:Thing ObjectSomeValuesFrom(:r :B)) \
         SubClassOf(ObjectSomeValuesFrom(:r :B) :X)",
    );
    let v = unlowered(&f);
    assert_eq!(v["schema_version"], 1);
    assert_eq!(v["classify_certified"], true);
    let rows = v["classify"].as_array().unwrap();
    assert_eq!(rows.len(), 1, "{v}");
    assert_eq!(rows[0]["site"], "top-lhs-nonatomic");
    assert_eq!(rows[0]["concept"], "∃r.B");
}

#[test]
fn a_fully_lowered_input_reports_nothing() {
    let f = Fixture::new(
        "clean",
        "SubClassOf(:A ObjectSomeValuesFrom(:r :B)) SubClassOf(ObjectSomeValuesFrom(:r :B) :X)",
    );
    let v = unlowered(&f);
    assert_eq!(v["classify_certified"], true);
    assert_eq!(v["classify"].as_array().unwrap().len(), 0, "{v}");
    assert_eq!(v["realize"].as_array().unwrap().len(), 0, "{v}");
}
