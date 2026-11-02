//! #409: `prove` with a class IRI that is not in the ontology is an error
//! (`class IRI not in ontology`, exit 1), as for `subclass`, never
//! "NOT entailed". `owl:Thing` / `owl:Nothing` are not in the named-class
//! vocabulary (unless declared), so they are refused the same way.
#![allow(clippy::unwrap_used)]

use std::path::PathBuf;
use std::process::Command;

const ONTO: &str = r"Prefix(:=<http://ex/#>)
Ontology(<http://ex/o>
  Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:Unused))
  SubClassOf(:A :B))
";

/// Writes the fixture to a per-test file (tests run in parallel).
struct Fixture(PathBuf);

impl Fixture {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "rustdl-prove-unknown-iri-{}-{tag}.ofn",
            std::process::id()
        ));
        std::fs::write(&path, ONTO).unwrap();
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

struct Run {
    code: i32,
    stdout: String,
    stderr: String,
}

/// Runs the binary with no inherited `RUSTDL_*` settings.
fn run(args: &[&str]) -> Run {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_rustdl"));
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("RUSTDL_") {
            cmd.env_remove(key);
        }
    }
    let out = cmd.args(args).output().unwrap();
    Run {
        code: out.status.code().unwrap_or(-1),
        stdout: String::from_utf8(out.stdout).unwrap(),
        stderr: String::from_utf8(out.stderr).unwrap(),
    }
}

/// Asserts `prove` refused the query because `missing` is not a class.
fn assert_refused(r: &Run, missing: &str) {
    assert_eq!(r.code, 1, "stdout: {}\nstderr: {}", r.stdout, r.stderr);
    assert!(r.stdout.is_empty(), "{}", r.stdout);
    assert!(
        r.stderr
            .contains(&format!("class IRI not in ontology: {missing}")),
        "{}",
        r.stderr
    );
}

#[test]
fn prove_refuses_an_unknown_sub_class() {
    let f = Fixture::new("sub");
    assert_refused(
        &run(&["prove", f.path(), "http://ex/#Aa", "http://ex/#B"]),
        "http://ex/#Aa",
    );
}

#[test]
fn prove_refuses_a_typo_d_tautology() {
    // `Aa ⊑ Aa` holds for any class; it used to print "NOT entailed".
    let f = Fixture::new("refl");
    assert_refused(
        &run(&["prove", f.path(), "http://ex/#Aa", "http://ex/#Aa"]),
        "http://ex/#Aa",
    );
}

#[test]
fn prove_json_refuses_an_unknown_sup_class() {
    let f = Fixture::new("sup-json");
    assert_refused(
        &run(&["prove", f.path(), "--json", "http://ex/#A", "http://ex/#Bb"]),
        "http://ex/#Bb",
    );
}

#[test]
fn prove_refuses_owl_thing() {
    // `A ⊑ owl:Thing` holds; it used to print "NOT entailed".
    let thing = "http://www.w3.org/2002/07/owl#Thing";
    let f = Fixture::new("thing");
    assert_refused(&run(&["prove", f.path(), "http://ex/#A", thing]), thing);
}

#[test]
fn prove_still_answers_for_declared_classes() {
    let f = Fixture::new("control");
    let unused = run(&["prove", f.path(), "http://ex/#Unused", "http://ex/#B"]);
    assert_eq!(unused.code, 0, "{}", unused.stderr);
    assert!(
        unused.stdout.starts_with("NOT entailed"),
        "{}",
        unused.stdout
    );
    let told = run(&["prove", f.path(), "http://ex/#A", "http://ex/#B"]);
    assert_eq!(told.code, 0, "{}", told.stderr);
    assert!(told.stdout.contains("A SubClassOf B"), "{}", told.stdout);
}
