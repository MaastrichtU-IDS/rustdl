//! Package 2 prerequisite (#363): the class-expression commands report a give-up
//! the way their named siblings do, and run under the same single-query node cap.
//!
//! - `incomplete` means only "a probe gave up" (#395); an answer off the pure-EL
//!   fast path is reported by `completeness_guaranteed` instead.
//! - On a give-up `sat-expr` / `subclass-expr` print `unknown` and exit 3 (as
//!   `sat` / `subclass`; with `--json` they exit 3 after printing), and
//!   `instances-expr` lists its sound members, warns and exits 0 (as `instances`).
//! - `sat-expr`, `subclass-expr`, `instances-expr` and `instances` hold
//!   `single_query_node_cap()`: with `RUSTDL_QUERY_MAX_NODES=1` and
//!   `RUSTDL_MAX_NODES` unset they give up, which a command keeping the
//!   classify-wide 50000 would not (#402's method).
//!
//! Each case runs the binary in its own process, so the `OnceLock`-cached caps
//! are read fresh every time.
#![allow(clippy::unwrap_used)]

use std::path::PathBuf;
use std::process::Command;

// Off EL (`∀s.E`); C and D each need an `:r`-successor, so every tableau probe
// on them trips a cap of 1. `a : C` gives the instance queries a member.
const OFF_EL: &str = r"Prefix(:=<http://ex/#>)
Ontology(<http://ex/exprgiveup>
  Declaration(Class(:C)) Declaration(Class(:D)) Declaration(Class(:E))
  Declaration(ObjectProperty(:r)) Declaration(ObjectProperty(:s))
  Declaration(NamedIndividual(:a))
  SubClassOf(:C ObjectSomeValuesFrom(:r :C))
  SubClassOf(:D ObjectSomeValuesFrom(:r :D))
  SubClassOf(:C ObjectAllValuesFrom(:s :E))
  ClassAssertion(:C :a)
)
";

// Pure EL and ABox-free: `sat-expr` / `subclass-expr` take the pure-EL fast
// path. (An ABox takes a query off it: `is_pure_el` counts assertions.)
const PURE_EL: &str = r"Prefix(:=<http://ex/#>)
Ontology(<http://ex/exprel>
  Declaration(Class(:C)) Declaration(Class(:D)) Declaration(ObjectProperty(:r))
  SubClassOf(:C ObjectSomeValuesFrom(:r :C))
)
";

// EL with an atomic class assertion: the instance query takes the realize
// saturation fast path, complete on its fragment.
const EL_ABOX: &str = r"Prefix(:=<http://ex/#>)
Ontology(<http://ex/exprelabox>
  Declaration(Class(:C)) Declaration(ObjectProperty(:r)) Declaration(NamedIndividual(:a))
  SubClassOf(:C ObjectSomeValuesFrom(:r :C))
  ClassAssertion(:C :a)
)
";

struct Fixture(PathBuf);

impl Fixture {
    fn new(src: &str, tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "rustdl-expr-give-up-{}-{tag}.ofn",
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

struct Run {
    code: i32,
    stdout: String,
    stderr: String,
}

impl Run {
    fn json(&self) -> serde_json::Value {
        serde_json::from_str(&self.stdout).unwrap_or_else(|e| panic!("{e}: {}", self.stdout))
    }
}

/// Runs the binary with exactly the given cap variables set (the others removed).
/// The pseudo-model prune is off so the instance queries reach their probes.
fn run(env: &[(&str, &str)], args: &[&str]) -> Run {
    let out = Command::new(env!("CARGO_BIN_EXE_rustdl"))
        .args(args)
        .env_remove("RUSTDL_MAX_NODES")
        .env_remove("RUSTDL_QUERY_MAX_NODES")
        .env("RUSTDL_PSEUDO_MODEL", "0")
        .envs(env.iter().copied())
        .output()
        .unwrap();
    Run {
        code: out.status.code().unwrap_or(-1),
        stdout: String::from_utf8(out.stdout).unwrap(),
        stderr: String::from_utf8(out.stderr).unwrap(),
    }
}

/// Reaches only commands that hold `single_query_node_cap()`.
const QUERY_CAP1: &[(&str, &str)] = &[("RUSTDL_QUERY_MAX_NODES", "1")];
const DEFAULTS: &[(&str, &str)] = &[];
const D: &str = "http://ex/#D";
const A: &str = "http://ex/#a";

#[test]
fn sat_expr_and_subclass_expr_print_unknown_on_a_give_up() {
    let f = Fixture::new(OFF_EL, "single-text");
    for args in [
        vec!["sat-expr", f.path(), ":C"],
        vec!["subclass-expr", f.path(), ":C", ":D"],
    ] {
        let r = run(QUERY_CAP1, &args);
        assert_eq!(r.stdout.trim(), "unknown", "{args:?}: {}", r.stdout);
        assert_eq!(r.code, 3, "{args:?}: {}", r.stderr);
        assert!(r.stderr.contains("RUSTDL_QUERY_MAX_NODES"), "{}", r.stderr);
    }
}

#[test]
fn sat_expr_and_subclass_expr_json_report_a_give_up_and_exit_3() {
    let f = Fixture::new(OFF_EL, "single-json");
    let r = run(QUERY_CAP1, &["sat-expr", "--json", f.path(), ":C"]);
    let j = r.json();
    assert_eq!(j["incomplete"], true, "{j}");
    assert_eq!(j["completeness_guaranteed"], false, "{j}");
    assert_eq!(j["satisfiable"], true, "the sound direction: {j}");
    assert_eq!(r.code, 3);
    let r = run(
        QUERY_CAP1,
        &["subclass-expr", "--json", f.path(), ":C", ":D"],
    );
    let j = r.json();
    assert_eq!(j["incomplete"], true, "{j}");
    assert_eq!(j["entailed"], false, "the sound direction: {j}");
    assert_eq!(r.code, 3);
}

/// The list answers, as `instances`: sound members, a warning, exit 0.
#[test]
fn instances_expr_and_instances_warn_on_a_give_up_and_exit_0() {
    let f = Fixture::new(OFF_EL, "list");
    for args in [
        vec!["instances-expr", f.path(), ":D"],
        vec!["instances", f.path(), D],
    ] {
        let r = run(QUERY_CAP1, &args);
        assert_eq!(r.code, 0, "{args:?}: {}", r.stderr);
        assert!(r.stdout.trim().is_empty(), "{args:?}: {}", r.stdout);
        assert!(
            r.stderr.contains("instance probe was cut")
                && r.stderr.contains("RUSTDL_QUERY_MAX_NODES"),
            "{args:?}: {}",
            r.stderr
        );
    }
    let r = run(QUERY_CAP1, &["instances-expr", "--json", f.path(), ":D"]);
    let j = r.json();
    assert_eq!(j["incomplete"], true, "{j}");
    assert_eq!(j["completeness_guaranteed"], false, "{j}");
    assert_eq!(r.code, 0);
}

/// The split itself: off EL with nothing cut, `incomplete` is false and only
/// `completeness_guaranteed` says the answer is not proven complete; there is no
/// warning and the exit code is 0.
#[test]
fn off_el_without_a_give_up_is_not_incomplete() {
    let f = Fixture::new(OFF_EL, "split");
    for (args, key, want) in [
        (
            vec!["sat-expr", "--json", f.path(), ":C"],
            "satisfiable",
            serde_json::json!(true),
        ),
        (
            vec!["subclass-expr", "--json", f.path(), ":C", ":D"],
            "entailed",
            serde_json::json!(false),
        ),
        (
            vec!["instances-expr", "--json", f.path(), ":C"],
            "instances",
            serde_json::json!([A]),
        ),
    ] {
        let r = run(DEFAULTS, &args);
        let j = r.json();
        assert_eq!(j[key], want, "{args:?}: {j}");
        assert_eq!(j["incomplete"], false, "{args:?}: {j}");
        assert_eq!(j["completeness_guaranteed"], false, "{args:?}: {j}");
        assert_eq!(r.code, 0, "{args:?}");
    }
    for args in [
        vec!["sat-expr", f.path(), ":C"],
        vec!["subclass-expr", f.path(), ":C", ":D"],
        vec!["instances-expr", f.path(), ":C"],
    ] {
        let r = run(DEFAULTS, &args);
        assert_eq!(r.code, 0, "{args:?}");
        assert!(
            !r.stderr.contains("incomplete") && !r.stderr.contains("cut"),
            "{args:?}: no give-up warning off EL: {}",
            r.stderr
        );
    }
}

/// On the complete fast paths with nothing cut, completeness is guaranteed.
#[test]
fn the_complete_fast_paths_guarantee_completeness() {
    let el = Fixture::new(PURE_EL, "el");
    let abox = Fixture::new(EL_ABOX, "el-abox");
    for args in [
        vec!["sat-expr", "--json", el.path(), ":C"],
        vec!["subclass-expr", "--json", el.path(), ":C", ":D"],
        vec!["instances-expr", "--json", abox.path(), ":C"],
    ] {
        let j = run(DEFAULTS, &args).json();
        assert_eq!(j["incomplete"], false, "{args:?}: {j}");
        assert_eq!(j["completeness_guaranteed"], true, "{args:?}: {j}");
    }
}
