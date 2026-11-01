//! #401 on the CLI: a single query answered with the `ABox` seed dropped, on the
//! strength of a wedge `Sat` for consistency, says so on stderr. The answer and
//! the exit code are unchanged, and `--json` carries
//! `relies_on_trusted_consistency` on the expression commands and `prove`.
//!
//! `run` sets `RUSTDL_QUERY_SEEDED_FIRST_MS=0` unless a test overrides it: on
//! this tiny fixture the seeded probe would answer first, and these tests are
//! about the skip.
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
        .env("RUSTDL_QUERY_SEEDED_FIRST_MS", "0")
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

/// #401 on `prove`: a "NOT entailed" reached by the skip carries the note and,
/// under `--json`, `relies_on_trusted_consistency: true`, separate from
/// `incomplete` and `status`. With `=0` neither appears.
#[test]
fn prove_reports_a_trusted_not_entailed() {
    let f = fixture("prove");
    let p = f.0.to_str().unwrap();
    let args = ["prove", p, "http://ex/#A", "http://ex/#C"];
    let r = run(&[], &args);
    assert_eq!(r.code, 0);
    assert!(r.stdout.contains("NOT entailed"), "{}", r.stdout);
    assert!(r.stderr.contains(NOTE), "{}", r.stderr);
    let off = run(&[("RUSTDL_QUERY_TBOX_ONLY", "0")], &args);
    assert!(off.stdout.contains("NOT entailed"), "{}", off.stdout);
    assert!(!off.stderr.contains(NOTE), "{}", off.stderr);

    let json = ["prove", "--json", p, "http://ex/#A", "http://ex/#C"];
    let r = run(&[], &json);
    let v: serde_json::Value = serde_json::from_str(&r.stdout).unwrap();
    assert_eq!(v["status"], "not-entailed");
    assert_eq!(v["incomplete"], false, "a trusted verdict is not a give-up");
    assert_eq!(v["relies_on_trusted_consistency"], true, "{}", r.stdout);
    assert!(r.stderr.contains(NOTE), "{}", r.stderr);
    let off = run(&[("RUSTDL_QUERY_TBOX_ONLY", "0")], &json);
    let v: serde_json::Value = serde_json::from_str(&off.stdout).unwrap();
    assert_eq!(v["status"], "not-entailed");
    assert_eq!(v["relies_on_trusted_consistency"], false);
}

/// An entailment `prove` shows never carries the flag: dropping the `ABox` can
/// only lose entailments, so one found without it holds regardless.
#[test]
fn prove_entailed_never_relies_on_trusted_consistency() {
    let f = fixture("prove-entailed");
    let p = f.0.to_str().unwrap();
    let r = run(&[], &["prove", "--json", p, "http://ex/#A", "http://ex/#B"]);
    let v: serde_json::Value = serde_json::from_str(&r.stdout).unwrap();
    assert_eq!(v["status"], "entailed", "{}", r.stdout);
    assert_eq!(v["relies_on_trusted_consistency"], false);
}

/// Seeded first, at the default budget: the seeded probe answers this fixture,
/// so the answer is unchanged and nothing rests on a trusted consistency.
#[test]
fn at_the_default_budget_the_seeded_probe_answers_first() {
    let f = fixture("seeded-first");
    let p = f.0.to_str().unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_rustdl"))
        .args(["sat", p, "http://ex/#A"])
        .env_remove("RUSTDL_QUERY_SEEDED_FIRST_MS")
        .output()
        .unwrap();
    assert_eq!(String::from_utf8(out.stdout).unwrap().trim(), "sat");
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(!stderr.contains(NOTE), "{stderr}");
}
