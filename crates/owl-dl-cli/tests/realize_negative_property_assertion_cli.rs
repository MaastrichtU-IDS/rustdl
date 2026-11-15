//! #536 on the CLI: an `ABox` made inconsistent by a `NegativeObjectPropertyAssertion`
//! (here `r(b, a)` with `¬r⁻(a, b)`, a direct clash) used to get a definite wrong `no`
//! from `instance`, empty output from `instances` / `instances-expr`, and empty types from
//! `realize`, all with exit 0. Each now refuses the inconsistent ontology (exit 1). The
//! consistent control (the edge reversed) still answers. `HermiT` and Konclude agree on
//! both files.
#![allow(clippy::unwrap_used)]

use std::path::PathBuf;
use std::process::Command;

const INCONSISTENT: &str = "ObjectPropertyAssertion(:r :b :a) \
                            NegativeObjectPropertyAssertion(ObjectInverseOf(:r) :a :b)";
const CONSISTENT: &str = "ObjectPropertyAssertion(:r :a :b) \
                          NegativeObjectPropertyAssertion(ObjectInverseOf(:r) :a :b)";

/// Writes the fixture to a per-test file (tests run in parallel).
struct Fixture(PathBuf);

impl Fixture {
    fn new(tag: &str, abox: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "rustdl-realize-npa-{}-{tag}.ofn",
            std::process::id()
        ));
        let src = format!(
            "Prefix(:=<http://ex/#>)\nOntology(<http://ex/o>\n\
             Declaration(ObjectProperty(:r)) Declaration(Class(:C)) Declaration(Class(:D))\n\
             Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))\n\
             ClassAssertion(:C :a)\n{abox}\n)\n"
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

/// (exit code, stdout).
fn run(args: &[&str]) -> (i32, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_rustdl"))
        .args(args)
        .output()
        .unwrap();
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8(out.stdout).unwrap(),
    )
}

#[test]
fn the_consistent_control_still_answers() {
    let f = Fixture::new("consistent", CONSISTENT);
    let p = f.path();
    assert_eq!(
        run(&["instance", p, "http://ex/#D", "http://ex/#b"]),
        (0, "no\n".to_string())
    );
    assert_eq!(
        run(&["instance", p, "http://ex/#C", "http://ex/#a"]),
        (0, "yes\n".to_string())
    );
    assert_eq!(run(&["realize", "--json", p]).0, 0);
}

#[test]
fn every_instance_surface_refuses_the_inconsistent_abox() {
    let f = Fixture::new("inconsistent", INCONSISTENT);
    let p = f.path();
    for args in [
        vec!["instance", p, "http://ex/#D", "http://ex/#b"],
        vec!["instances", p, "http://ex/#D"],
        vec!["instances-expr", p, "D"],
        vec!["realize", p],
        vec!["realize", "--json", p],
    ] {
        let (code, stdout) = run(&args);
        assert_eq!(
            code, 1,
            "{args:?}: must refuse (exit 1), got {code} with {stdout:?}"
        );
        assert!(
            stdout.trim().is_empty(),
            "{args:?}: printed an answer: {stdout:?}"
        );
    }
}
