//! #427: the named-class commands answer `owl:Thing` / `owl:Nothing` as the
//! built-in ⊤ / ⊥, declared or not. On main they were refused when undeclared
//! (`class IRI not in ontology`, exit 1) and answered wrongly when declared
//! (`sat owl:Nothing` → `sat`, `subclass A owl:Thing` → `no`).
#![allow(clippy::unwrap_used)]

use std::path::PathBuf;
use std::process::Command;

const THING: &str = "http://www.w3.org/2002/07/owl#Thing";
const NOTHING: &str = "http://www.w3.org/2002/07/owl#Nothing";
const A: &str = "http://ex/#A";
const U: &str = "http://ex/#U";
const IND: &str = "http://ex/#a";

const BODY: &str = "Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:U))
  Declaration(NamedIndividual(:a))
  SubClassOf(:A :B) SubClassOf(:U owl:Nothing) ClassAssertion(:A :a)";

const INCONSISTENT: &str = "Declaration(Class(:A)) Declaration(Class(:B))
  Declaration(NamedIndividual(:a))
  DisjointClasses(:A :B) ClassAssertion(:A :a) ClassAssertion(:B :a)";

/// Writes the fixture to a per-test file (tests run in parallel).
struct Fixture(PathBuf);

impl Fixture {
    fn new(tag: &str, body: &str, declared: bool) -> Self {
        let decls = if declared {
            "Declaration(Class(owl:Thing)) Declaration(Class(owl:Nothing))"
        } else {
            ""
        };
        let path = std::env::temp_dir().join(format!(
            "rustdl-builtin-class-{}-{tag}-{declared}.ofn",
            std::process::id()
        ));
        let src = format!(
            "Prefix(:=<http://ex/#>)\nPrefix(owl:=<http://www.w3.org/2002/07/owl#>)\n\
             Ontology(<http://ex/o>\n{decls}\n{body}\n)\n"
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

/// Runs and asserts exit 0, returning stdout.
fn ok(args: &[&str]) -> String {
    let r = run(args);
    assert_eq!(
        r.code, 0,
        "{args:?}\nstdout: {}\nstderr: {}",
        r.stdout, r.stderr
    );
    r.stdout
}

#[test]
fn verdict_commands_answer_the_built_ins() {
    for declared in [false, true] {
        let f = Fixture::new("verdicts", BODY, declared);
        let p = f.path();
        for (args, expected) in [
            (vec!["sat", p, THING], "sat"),
            (vec!["sat", p, NOTHING], "unsat"),
            (vec!["subclass", p, A, THING], "yes"),
            (vec!["subclass", p, NOTHING, A], "yes"),
            (vec!["subclass", p, U, NOTHING], "yes"),
            (vec!["subclass", p, A, NOTHING], "no"),
            (vec!["subclass", p, THING, NOTHING], "no"),
            (vec!["subclass", "--saturation-only", p, A, THING], "yes"),
            (vec!["instance", p, THING, IND], "yes"),
            (vec!["instance", p, NOTHING, IND], "no"),
            (vec!["instances", p, THING], IND),
            (vec!["instances", p, NOTHING], ""),
        ] {
            assert_eq!(ok(&args).trim(), expected, "declared={declared}: {args:?}");
        }
        let explained = ok(&["explain", p, A, THING]);
        assert!(
            explained.contains(": yes — answered by structure"),
            "{explained}"
        );
    }
}

#[test]
fn a_declared_built_in_adds_no_class_to_the_taxonomy() {
    let count = |declared: bool| {
        let f = Fixture::new("classify", BODY, declared);
        let out = ok(&["classify", f.path()]);
        out.lines()
            .find(|l| l.starts_with("# classes:"))
            .unwrap()
            .to_owned()
    };
    assert_eq!(count(true), count(false));
}

#[test]
fn prove_and_justify_the_built_in_forms() {
    let f = Fixture::new("prove", BODY, true);
    let p = f.path();
    let tautology = ok(&["prove", p, A, THING]);
    assert!(tautology.contains("holds in every ontology"), "{tautology}");
    assert!(!tautology.contains("step proof unavailable"), "{tautology}");

    let json: serde_json::Value =
        serde_json::from_str(&ok(&["prove", "--json", p, A, THING])).unwrap();
    assert_eq!(json["status"], "entailed");
    assert_eq!(json["justification_minimal"], true);
    assert_eq!(json["incomplete"], false);

    // `U ⊑ owl:Nothing`: a step proof, and it checks.
    let r = run(&["prove", "--verify-proof", p, U, NOTHING]);
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert!(r.stdout.starts_with("# step proof"), "{}", r.stdout);
    assert!(r.stderr.contains("proof verified OK"), "{}", r.stderr);

    let justified = ok(&["justify", p, "subclass", A, THING]);
    assert!(justified.contains("(0 axioms) — minimal"), "{justified}");
}

/// #313 / #419: on an inconsistent ontology every instance query errors, and ⊤ is
/// no exception.
#[test]
fn instance_of_owl_thing_still_errors_on_an_inconsistent_ontology() {
    let f = Fixture::new("incons", INCONSISTENT, false);
    let r = run(&["instance", f.path(), THING, IND]);
    assert_ne!(r.code, 0, "{}", r.stdout);
    assert!(
        r.stderr.contains("ontology is inconsistent"),
        "{}",
        r.stderr
    );
    assert_eq!(ok(&["sat", f.path(), THING]).trim(), "unsat");
    assert_eq!(ok(&["subclass", f.path(), THING, NOTHING]).trim(), "yes");
}
