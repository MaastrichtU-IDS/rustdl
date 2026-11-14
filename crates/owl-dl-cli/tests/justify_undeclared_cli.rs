//! #520: `justify`, `repair` and `report` on queries whose class or individual has no
//! `Declaration`. They used to abort with "IRI not in ontology" when the search tested a
//! subset without the entity's axioms. The API's exact sets are pinned in
//! `owl-dl-reasoner/tests/justify_undeclared_entities.rs`.
#![allow(clippy::unwrap_used)]

use std::path::PathBuf;
use std::process::Command;

struct Out {
    code: i32,
    stdout: String,
    stderr: String,
}

/// The fixture file, removed on drop (also when an assertion fails).
struct Fixture(PathBuf);

impl Fixture {
    fn new(tag: &str, body: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "rustdl-justify-undeclared-{}-{tag}.ofn",
            std::process::id()
        ));
        let src = format!(
            "Prefix(:=<http://ex/#>)\nPrefix(owl:=<http://www.w3.org/2002/07/owl#>)\n\
             Ontology(<http://ex/o>\n{body}\n)\n"
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

/// Runs `rustdl` with every `RUSTDL_*` variable cleared.
fn run(args: &[&str]) -> Out {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_rustdl"));
    for (var, _) in std::env::vars_os() {
        if var.to_string_lossy().starts_with("RUSTDL_") {
            cmd.env_remove(var);
        }
    }
    let out = cmd.args(args).output().unwrap();
    Out {
        code: out.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    }
}

const A: &str = "http://ex/#a";
const C: &str = "http://ex/#C";

/// C1 at the CLI: no declarations at all.
#[test]
fn justify_and_repair_answer_an_undeclared_instance_query() {
    let f = Fixture::new(
        "chain",
        "SubClassOf(:A :B) SubClassOf(:B :C) ClassAssertion(:A :a)",
    );
    let r = run(&["justify", f.path(), "instance", A, C]);
    assert_eq!(r.code, 0, "{}\n{}", r.stdout, r.stderr);
    assert!(
        r.stdout.contains("justification (3 axioms)"),
        "{}",
        r.stdout
    );
    let r = run(&["repair", f.path(), "instance", A, C]);
    assert_eq!(r.code, 0, "{}\n{}", r.stdout, r.stderr);
    assert!(r.stdout.contains("3 minimal repair(s)"), "{}", r.stdout);
}

/// C7 at the CLI: an IRI the ontology does not have is still main's error.
#[test]
fn a_typo_is_still_an_error() {
    let f = Fixture::new("typo", "SubClassOf(:A :B) ClassAssertion(:A :a)");
    for cmd in ["justify", "repair"] {
        let r = run(&[cmd, f.path(), "instance", A, "http://ex/#Typo"]);
        assert_ne!(r.code, 0, "{cmd}: {}", r.stdout);
        assert!(
            r.stderr
                .contains("class IRI not in ontology: http://ex/#Typo"),
            "{cmd}: {}",
            r.stderr
        );
    }
}

/// C11 at the CLI (the stated behaviour change): `C` is outside the query's ⊥-module
/// but in the ontology. Main reported "class IRI not in ontology"; now "not entailed".
#[test]
fn an_entity_outside_the_module_is_not_entailed() {
    let f = Fixture::new("module", "SubClassOf(:D :C) ClassAssertion(:X :a)");
    let r = run(&["justify", f.path(), "instance", A, C]);
    assert_eq!(r.code, 0, "{}\n{}", r.stdout, r.stderr);
    assert!(r.stdout.starts_with("not entailed"), "{}", r.stdout);
    let r = run(&["repair", f.path(), "instance", A, C]);
    assert_eq!(r.code, 0, "{}\n{}", r.stdout, r.stderr);
    assert!(r.stdout.contains("not entailed"), "{}", r.stdout);
}

/// C14: `report` justifies every unsatisfiable class through the prepared justifier
/// and repair; on main it aborted with "justify root: class IRI not in ontology".
#[test]
fn report_justifies_an_undeclared_unsatisfiable_class() {
    let f = Fixture::new(
        "report",
        "SubClassOf(:V :U) SubClassOf(:U owl:Nothing) SubClassOf(:A :B)",
    );
    let r = run(&["report", f.path()]);
    assert_eq!(r.code, 0, "{}\n{}", r.stdout, r.stderr);
    assert!(!r.stderr.contains("not in ontology"), "{}", r.stderr);
    assert!(
        r.stdout.contains('V') && r.stdout.contains('U'),
        "{}",
        r.stdout
    );
}
