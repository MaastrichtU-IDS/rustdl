//! #401 on the CLI: a single query answered with the `ABox` seed dropped, on the
//! strength of a wedge `Sat` for consistency, says so on stderr. The answer and
//! the exit code are unchanged, and `--json` carries
//! `relies_on_trusted_consistency` on the expression commands.
#![allow(clippy::unwrap_used)]

use std::path::PathBuf;
use std::process::Command;

// Off pure EL (the union), with an `ABox` and no nominal.
const SRC: &str = r"Prefix(:=<http://ex/#>)
Ontology(<http://ex/querytboxonly>
  Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C)) Declaration(Class(:D))
  Declaration(ObjectProperty(:r))
  Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))
  SubClassOf(:A ObjectUnionOf(:C :D))
  SubClassOf(:C :B) SubClassOf(:D :B)
  ClassAssertion(:A :a) ObjectPropertyAssertion(:r :a :b)
)
";

struct Fixture(PathBuf);

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn fixture(tag: &str) -> Fixture {
    let path = std::env::temp_dir().join(format!(
        "rustdl-query-tbox-only-{}-{tag}.ofn",
        std::process::id()
    ));
    std::fs::write(&path, SRC).unwrap();
    Fixture(path)
}

struct Run {
    code: i32,
    stdout: String,
    stderr: String,
}

fn run(env: &[(&str, &str)], args: &[&str]) -> Run {
    let out = Command::new(env!("CARGO_BIN_EXE_rustdl"))
        .args(args)
        .envs(env.iter().copied())
        .output()
        .unwrap();
    Run {
        code: out.status.code().unwrap_or(-1),
        stdout: String::from_utf8(out.stdout).unwrap(),
        stderr: String::from_utf8(out.stderr).unwrap(),
    }
}

const NOTE: &str = "answered without the ABox";

#[test]
fn sat_notes_a_trusted_consistency() {
    let f = fixture("sat");
    let p = f.0.to_str().unwrap();
    let r = run(&[], &["sat", p, "http://ex/#A"]);
    assert_eq!(r.stdout.trim(), "sat");
    assert_eq!(r.code, 0);
    assert!(r.stderr.contains(NOTE), "{}", r.stderr);
    let off = run(
        &[("RUSTDL_QUERY_TBOX_ONLY", "0")],
        &["sat", p, "http://ex/#A"],
    );
    assert_eq!(off.stdout.trim(), "sat");
    assert!(!off.stderr.contains(NOTE), "{}", off.stderr);
}

#[test]
fn subclass_and_explain_note_it_too() {
    let f = fixture("subclass");
    let p = f.0.to_str().unwrap();
    let r = run(&[], &["subclass", p, "http://ex/#A", "http://ex/#C"]);
    assert_eq!(r.stdout.trim(), "no");
    assert!(r.stderr.contains(NOTE), "{}", r.stderr);
    let e = run(&[], &["explain", p, "http://ex/#A", "http://ex/#C"]);
    assert_eq!(e.code, 0);
    assert!(e.stderr.contains(NOTE), "{}", e.stderr);
}

#[test]
fn sat_expr_json_carries_the_field() {
    let f = fixture("satexpr");
    let p = f.0.to_str().unwrap();
    let r = run(&[], &["sat-expr", "--json", p, ":A"]);
    let v: serde_json::Value = serde_json::from_str(&r.stdout).unwrap();
    assert_eq!(v["satisfiable"], true);
    assert_eq!(v["relies_on_trusted_consistency"], true, "{}", r.stdout);
    let off = run(
        &[("RUSTDL_QUERY_TBOX_ONLY", "0")],
        &["sat-expr", "--json", p, ":A"],
    );
    let v: serde_json::Value = serde_json::from_str(&off.stdout).unwrap();
    assert_eq!(v["relies_on_trusted_consistency"], false);
}
