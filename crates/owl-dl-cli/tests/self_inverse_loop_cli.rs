//! #437: `consistent` exited 1 with "tableau bailed out without a verdict" on these
//! two ontologies. The main tableau never recognised the self-loop it added for
//! `ObjectHasSelf(ObjectInverseOf(:r1))`, so it added it forever. Konclude and HermiT:
//! A is consistent, B (A plus one axiom) is inconsistent.
#![allow(clippy::unwrap_used)]

use std::path::PathBuf;
use std::process::Command;

const REPRO_A: &str = "Prefix(:=<http://gen.rustdl/o#>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Ontology(<http://gen.rustdl/o>
Declaration(Class(:C1)) Declaration(Class(:C2)) Declaration(Class(:C4)) Declaration(Class(:C5))
Declaration(ObjectProperty(:r0)) Declaration(ObjectProperty(:r1))
SubClassOf(ObjectSomeValuesFrom(:r1 owl:Thing) :C4)
DisjointUnion(:C2 :C1 :C4)
SubClassOf(:C5 :C4)
EquivalentClasses(:C5 ObjectAllValuesFrom(:r1 ObjectUnionOf(:C5 :C2)))
SubClassOf(:C2 ObjectSomeValuesFrom(:r0 ObjectHasSelf(ObjectInverseOf(:r1))))
";

/// Writes the fixture to a per-test file (tests run in parallel).
struct Fixture(PathBuf);

impl Fixture {
    fn new(tag: &str, src: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "rustdl-self-inverse-loop-{}-{tag}.ofn",
            std::process::id()
        ));
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

/// Runs the binary with no inherited `RUSTDL_*` settings: (exit code, stdout, stderr).
fn run(args: &[&str]) -> (i32, String, String) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_rustdl"));
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("RUSTDL_") {
            cmd.env_remove(key);
        }
    }
    let out = cmd.args(args).output().unwrap();
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8(out.stdout).unwrap(),
        String::from_utf8(out.stderr).unwrap(),
    )
}

fn consistent_json(path: &str) -> serde_json::Value {
    let (code, stdout, stderr) = run(&["consistent", "--json", path]);
    assert_eq!(code, 0, "stdout: {stdout}\nstderr: {stderr}");
    serde_json::from_str(&stdout).unwrap()
}

#[test]
fn consistent_answers_both_reproducers() {
    let a = Fixture::new("a", &format!("{REPRO_A})\n"));
    let j = consistent_json(a.path());
    assert_eq!(j["consistent"], true);
    assert_eq!(j["incomplete"], false);

    let b = Fixture::new(
        "b",
        &format!("{REPRO_A}EquivalentClasses(:C1 ObjectSomeValuesFrom(:r0 :C4))\n)\n"),
    );
    let j = consistent_json(b.path());
    assert_eq!(j["consistent"], false);
    let (code, stdout, stderr) = run(&["consistent", b.path()]);
    assert_eq!(code, 0, "{stderr}");
    assert!(stdout.contains("inconsistent"), "{stdout}");
}

#[test]
fn sat_answers_every_class_of_reproducer_a() {
    let a = Fixture::new("a-sat", &format!("{REPRO_A})\n"));
    for (class, want) in [("C1", "unsat"), ("C2", "sat"), ("C4", "sat"), ("C5", "sat")] {
        let (code, stdout, stderr) = run(&["sat", a.path(), &format!("http://gen.rustdl/o#{class}")]);
        assert_eq!(code, 0, "{class}: {stderr}");
        assert_eq!(stdout.trim(), want, "{class}");
    }
}
