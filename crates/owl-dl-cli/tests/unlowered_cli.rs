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
    unlowered_with(f, &[])
}

/// `rustdl unlowered` with every inherited `RUSTDL_*` removed, then `env` set on the
/// child only.
fn unlowered_with(f: &Fixture, env: &[(&str, &str)]) -> serde_json::Value {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_rustdl"));
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("RUSTDL_") {
            cmd.env_remove(key);
        }
    }
    cmd.envs(env.iter().copied());
    let out = cmd.arg("unlowered").arg(&f.0).output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}

/// A certified drop is reported, with its site and the dropped construct. No EL shape
/// drops anything on main since #445, so the D10 chokepoint is switched off on the child
/// (`RUSTDL_EL_BOT_FILLER=0`), which leaves `A ⊑ ∃r.⊥` unlowered.
#[test]
fn reports_a_certified_drop() {
    let f = Fixture::new("bot", "SubClassOf(:A ObjectSomeValuesFrom(:r owl:Nothing))");
    let v = unlowered_with(&f, &[("RUSTDL_EL_BOT_FILLER", "0")]);
    assert_eq!(v["schema_version"], 1);
    assert_eq!(v["classify_certified"], true);
    let rows = v["classify"].as_array().unwrap();
    assert_eq!(rows.len(), 1, "{v}");
    assert_eq!(rows[0]["site"], "rhs-exists-dropped");
    assert_eq!(rows[0]["concept"], "∃r.⊥");
}

/// #445: `⊤ ⊑ ∃r.B`, the drop this diagnostic found on main, lowers in full.
#[test]
fn the_top_lhs_existential_reports_nothing() {
    let f = Fixture::new(
        "top",
        "SubClassOf(owl:Thing ObjectSomeValuesFrom(:r :B)) \
         SubClassOf(ObjectSomeValuesFrom(:r :B) :X)",
    );
    let v = unlowered(&f);
    assert_eq!(v["classify_certified"], true);
    assert_eq!(v["classify"].as_array().unwrap().len(), 0, "{v}");
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
