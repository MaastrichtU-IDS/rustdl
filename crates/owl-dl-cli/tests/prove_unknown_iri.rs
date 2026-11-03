//! #409: `prove` with a class IRI that is not in the ontology is an error
//! (`class IRI not in ontology`, exit 1), as for `subclass`, never
//! "NOT entailed". `owl:Thing` / `owl:Nothing` are the built-in ⊤ / ⊥ (#427), so
//! they are answered, but a typo beside one is still refused.
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
fn prove_answers_owl_thing_but_refuses_a_typo_beside_it() {
    // #427: `A ⊑ owl:Thing` holds in every ontology. It used to be refused (and,
    // with owl:Thing declared, printed "NOT entailed").
    let thing = "http://www.w3.org/2002/07/owl#Thing";
    let nothing = "http://www.w3.org/2002/07/owl#Nothing";
    let f = Fixture::new("thing");
    let held = run(&["prove", f.path(), "http://ex/#A", thing]);
    assert_eq!(held.code, 0, "{}", held.stderr);
    assert!(
        held.stdout.contains("holds in every ontology"),
        "{}",
        held.stdout
    );
    // The tautology shortcut must not swallow the #409 typo check.
    assert_refused(
        &run(&["prove", f.path(), "http://ex/#Aa", thing]),
        "http://ex/#Aa",
    );
    assert_refused(
        &run(&["prove", f.path(), nothing, "http://ex/#Aa"]),
        "http://ex/#Aa",
    );
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
