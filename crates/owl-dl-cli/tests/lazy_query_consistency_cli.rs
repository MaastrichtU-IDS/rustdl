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
        vec!["sat-expr", path, "<http://ex/#Y>"],
        vec!["subclass-expr", path, "<http://ex/#Y>", "owl:Thing"],
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

const TRACE: &str = "lazy consistency (#369)";

/// The check runs after a positive answer only: with `RUSTDL_TRACE` it logs one
/// line, and a negative answer logs none.
#[test]
fn the_check_runs_only_after_a_positive_answer() {
    let f = Fixture::new("trace", 1);
    let trace = [("RUSTDL_TRACE", "1")];
    let neg = run(
        &["instance", f.path(), "http://ex/#X", "http://ex/#a"],
        &trace,
    );
    assert_eq!(neg.stdout, "no\n", "{}", neg.stderr);
    assert!(!neg.stderr.contains(TRACE), "{}", neg.stderr);
    let pos = run(
        &["instance", f.path(), "http://ex/#Y", "http://ex/#a"],
        &trace,
    );
    assert_eq!(pos.stdout, "yes\n", "{}", pos.stderr);
    assert!(pos.stderr.contains(TRACE), "{}", pos.stderr);
}

/// `justify` and `repair` skip the check: their oracle maps `Inconsistent` to the
/// same verdict as the vacuous answer, so it could only cost time there.
#[test]
fn justify_never_runs_the_check() {
    for (min, tag) in [(2, "justify-incons"), (1, "justify-ctl")] {
        let f = Fixture::new(tag, min);
        let r = run(
            &[
                "justify",
                f.path(),
                "instance",
                "http://ex/#a",
                "http://ex/#Y",
            ],
            &[("RUSTDL_TRACE", "1")],
        );
        assert!(!r.stderr.contains(TRACE), "min {min}: {}", r.stderr);
    }
}

/// The #369 follow-up's surfaces, on `T2` plus property assertions and property
/// disjointness: exit 1 when the wedge finds the inconsistency; with the wedge
/// off, the answer is kept and `consistency_undetermined` is in `--json`.
#[test]
fn followup_surfaces_error_or_flag() {
    let path = std::env::temp_dir().join(format!(
        "rustdl-lazy-consistency-{}-props.ofn",
        std::process::id()
    ));
    let src = T2.replace("MIN", "2").replace(
        "  ClassAssertion(:Y :a) ClassAssertion(:Y :c)",
        "  ClassAssertion(:Y :a) ClassAssertion(:Y :c)\n  \
         Declaration(ObjectProperty(:p)) Declaration(ObjectProperty(:q))\n  \
         ObjectPropertyAssertion(:p :a :c) DisjointObjectProperties(:p :q)",
    );
    std::fs::write(&path, src).unwrap();
    let f = path.to_str().unwrap();
    for args in [vec!["property-values", f], vec!["disjoint", f]] {
        let r = run(&args, &[]);
        assert_eq!(r.code, 1, "{args:?}: {}", r.stderr);
        assert!(r.stderr.contains(INCONSISTENT), "{args:?}: {}", r.stderr);
    }
    let wedge_off = [("RUSTDL_WEDGE_CONSISTENCY", "0")];
    for args in [
        vec!["property-values", f, "--json"],
        vec!["disjoint", f, "--json"],
        vec!["sat-expr", f, "--json", "<http://ex/#Y>"],
        vec!["subclass-expr", f, "--json", "<http://ex/#Y>", "owl:Thing"],
    ] {
        let r = run(&args, &wedge_off);
        let v: serde_json::Value = serde_json::from_str(&r.stdout)
            .unwrap_or_else(|e| panic!("{args:?}: {e}: {} / {}", r.stdout, r.stderr));
        assert_eq!(
            v["consistency_undetermined"], true,
            "{args:?}: {}",
            r.stdout
        );
    }
    let _ = std::fs::remove_file(&path);
}

/// #369 follow-up: as `the_check_runs_only_after_a_positive_answer`, for
/// `subclass-expr` (the light route).
#[test]
fn subclass_expr_checks_only_after_a_positive_answer() {
    let f = Fixture::new("trace-expr", 1);
    let trace = [("RUSTDL_TRACE", "1")];
    let neg = run(
        &["subclass-expr", f.path(), "owl:Thing", "<http://ex/#Y>"],
        &trace,
    );
    assert_eq!(neg.stdout, "no\n", "{}", neg.stderr);
    assert!(!neg.stderr.contains(TRACE), "{}", neg.stderr);
    let pos = run(
        &["subclass-expr", f.path(), "<http://ex/#Y>", "owl:Thing"],
        &trace,
    );
    assert_eq!(pos.stdout, "yes\n", "{}", pos.stderr);
    assert!(pos.stderr.contains(TRACE), "{}", pos.stderr);
    // A negative sat-expr ("satisfiable") and an empty property-values run none.
    for args in [
        vec!["sat-expr", f.path(), "<http://ex/#Y>"],
        vec!["property-values", f.path()],
    ] {
        let r = run(&args, &trace);
        assert_eq!(r.code, 0, "{args:?}: {}", r.stderr);
        assert!(!r.stderr.contains(TRACE), "{args:?}: {}", r.stderr);
    }
}
