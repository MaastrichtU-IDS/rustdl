//! #369: the CLI query surfaces error on an inconsistency only the tableau finds
//! (exit 1, "ontology is inconsistent"), and report a positive answer whose KB
//! consistency stayed undetermined (`consistency_undetermined` in `--json`, one
//! stderr warning in text mode), without changing the answer or the exit code.
#![allow(clippy::unwrap_used)]

use std::path::PathBuf;
use std::process::Command;

/// The TBox-only nominal `{b} ⊑ ≥2 r.⊤` with `r` functional: `b` cannot exist,
/// so the KB is inconsistent, and only the wedge or the tableau sees it.
const T2: &str = r"Prefix(:=<http://ex/#>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Ontology(<http://ex/t2>
  Declaration(Class(:X)) Declaration(Class(:Y)) Declaration(ObjectProperty(:r))
  Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b)) Declaration(NamedIndividual(:c))
  SubClassOf(ObjectOneOf(:b) ObjectMinCardinality(MIN :r owl:Thing))
  FunctionalObjectProperty(:r)
  ClassAssertion(:Y :a) ClassAssertion(:Y :c)
)
";

const INCONSISTENT: &str = "ontology is inconsistent";
const UNDETERMINED: &str = "consistency could not be determined";

/// Writes the fixture to a per-test file (tests run in parallel).
struct Fixture(PathBuf);

impl Fixture {
    /// `min` 2 is inconsistent, 1 consistent.
    fn new(tag: &str, min: u32) -> Self {
        let path = std::env::temp_dir().join(format!(
            "rustdl-lazy-consistency-{}-{tag}.ofn",
            std::process::id()
        ));
        std::fs::write(&path, T2.replace("MIN", &min.to_string())).unwrap();
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

/// Runs the binary with no inherited `RUSTDL_*` settings, plus `env`.
fn run(args: &[&str], env: &[(&str, &str)]) -> Run {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_rustdl"));
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("RUSTDL_") {
            cmd.env_remove(key);
        }
    }
    let out = cmd.args(args).envs(env.iter().copied()).output().unwrap();
    Run {
        code: out.status.code().unwrap_or(-1),
        stdout: String::from_utf8(out.stdout).unwrap(),
        stderr: String::from_utf8(out.stderr).unwrap(),
    }
}

fn surfaces(path: &str) -> Vec<Vec<&str>> {
    vec![
        vec!["instance", path, "http://ex/#X", "http://ex/#a"],
        vec!["instances", path, "http://ex/#X"],
        vec!["instances-expr", path, "<http://ex/#X>"],
        vec!["individuals", path],
    ]
}

#[test]
fn every_surface_exits_1_on_a_tableau_only_inconsistency() {
    let f = Fixture::new("incons", 2);
    for args in surfaces(f.path()) {
        let r = run(&args, &[]);
        assert_eq!(r.code, 1, "{args:?}: {}", r.stderr);
        assert!(r.stderr.contains(INCONSISTENT), "{args:?}: {}", r.stderr);
    }
}

/// Not wired yet: they would newly pay a prepare, so a follow-up prices them
/// with `materialize_*` and `property-values`. Until then, main's answers.
#[test]
fn sat_expr_and_subclass_expr_are_unchanged() {
    let f = Fixture::new("expr", 2);
    let r = run(&["sat-expr", f.path(), "<http://ex/#Y>"], &[]);
    assert_eq!(
        (r.code, r.stdout.as_str()),
        (0, "unsatisfiable\n"),
        "{}",
        r.stderr
    );
    let r = run(
        &["subclass-expr", f.path(), "<http://ex/#Y>", "owl:Thing"],
        &[],
    );
    assert_eq!((r.code, r.stdout.as_str()), (0, "yes\n"), "{}", r.stderr);
}

#[test]
fn a_consistent_kb_answers_without_a_warning() {
    let f = Fixture::new("ctl", 1);
    let r = run(&["instances", f.path(), "http://ex/#Y"], &[]);
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert_eq!(r.stdout, "http://ex/#a\nhttp://ex/#c\n");
    assert!(!r.stderr.contains(UNDETERMINED), "{}", r.stderr);
}

#[test]
fn an_undetermined_kb_keeps_the_answer_and_warns() {
    let f = Fixture::new("undet", 2);
    let wedge_off = [("RUSTDL_WEDGE_CONSISTENCY", "0")];
    let r = run(
        &["instance", f.path(), "http://ex/#X", "http://ex/#a"],
        &wedge_off,
    );
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert_eq!(r.stdout, "yes\n");
    assert!(r.stderr.contains(UNDETERMINED), "{}", r.stderr);
    for args in [
        vec!["instances-expr", f.path(), "--json", "<http://ex/#X>"],
        vec!["individuals", f.path(), "--json"],
    ] {
        let r = run(&args, &wedge_off);
        assert_eq!(r.code, 0, "{args:?}: {}", r.stderr);
        let v: serde_json::Value = serde_json::from_str(&r.stdout).unwrap();
        assert_eq!(
            v["consistency_undetermined"], true,
            "{args:?}: {}",
            r.stdout
        );
    }
    // The opt-in tail decides it.
    let r = run(
        &["instance", f.path(), "http://ex/#X", "http://ex/#a"],
        &[
            ("RUSTDL_WEDGE_CONSISTENCY", "0"),
            ("RUSTDL_QUERY_LAZY_CONSISTENCY_TAIL", "1"),
        ],
    );
    assert_eq!(r.code, 1, "{}", r.stderr);
    assert!(r.stderr.contains(INCONSISTENT), "{}", r.stderr);
}

#[test]
fn the_flag_reverts() {
    let f = Fixture::new("off", 2);
    let r = run(
        &["instance", f.path(), "http://ex/#X", "http://ex/#a"],
        &[("RUSTDL_QUERY_LAZY_CONSISTENCY", "0")],
    );
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert_eq!(r.stdout, "yes\n");
}
