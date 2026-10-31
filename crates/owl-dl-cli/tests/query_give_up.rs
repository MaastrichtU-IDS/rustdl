//! #363 A2: a query whose tableau probe gives up must not print a definite
//! answer.
//!
//! `sat`, `subclass`, `explain` and `instance` used to fold a live-node-cap
//! trip (`RUSTDL_MAX_NODES`) into "sat" / "no", exit 0, with nothing telling
//! the caller there was no verdict. They now print `unknown` and exit 3, the
//! convention `consistent` already uses for a give-up. `instances` and
//! `realize` print their (sound) members and warn on stderr.
//!
//! Each case runs the binary in its own process, so the `OnceLock`-cached
//! `RUSTDL_MAX_NODES` is read fresh every time. The control arm sets the cap to
//! `0` (disabled) and must give the definite answer.
#![allow(clippy::unwrap_used)]

use std::path::PathBuf;
use std::process::Command;

// Satisfiable everywhere and no subsumption between C and D, but each class
// generates an `:r`-successor, so any tableau probe on C or D needs a node
// beyond the root: with `RUSTDL_MAX_NODES=1` the probe trips the cap before a
// verdict. The `∀s.E` axiom takes the ontology out of the EL fragment, so the
// saturation fast path cannot answer and the tableau runs.
const SRC: &str = r"Prefix(:=<http://ex/#>)
Ontology(<http://ex/querygiveup>
  Declaration(Class(:C)) Declaration(Class(:D)) Declaration(Class(:E))
  Declaration(ObjectProperty(:r)) Declaration(ObjectProperty(:s))
  Declaration(NamedIndividual(:a))
  SubClassOf(:C ObjectSomeValuesFrom(:r :C))
  SubClassOf(:D ObjectSomeValuesFrom(:r :D))
  SubClassOf(:C ObjectAllValuesFrom(:s :E))
  ClassAssertion(:C :a)
)
";

/// Writes the fixture to a per-test file (tests run in parallel).
struct Fixture(PathBuf);

impl Fixture {
    fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn fixture_named(tag: &str) -> Fixture {
    let path = std::env::temp_dir().join(format!(
        "rustdl-query-give-up-{}-{tag}.ofn",
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

fn run(cap: &str, args: &[&str]) -> Run {
    run_with(cap, &[], args)
}

fn run_with(cap: &str, extra_env: &[(&str, &str)], args: &[&str]) -> Run {
    let out = Command::new(env!("CARGO_BIN_EXE_rustdl"))
        .args(args)
        .envs(extra_env.iter().copied())
        .env("RUSTDL_MAX_NODES", cap)
        // The pseudo-model prune can answer "not an instance" without a probe;
        // turn it off so the instance probe actually runs and can be cut.
        .env("RUSTDL_PSEUDO_MODEL", "0")
        .output()
        .unwrap();
    Run {
        code: out.status.code().unwrap_or(-1),
        stdout: String::from_utf8(out.stdout).unwrap(),
        stderr: String::from_utf8(out.stderr).unwrap(),
    }
}

const C: &str = "http://ex/#C";
const D: &str = "http://ex/#D";
const A: &str = "http://ex/#a";

#[test]
fn sat_prints_unknown_when_the_probe_hits_the_node_cap() {
    let f = fixture_named("sat");
    let p = f.path().to_str().unwrap();
    let capped = run("1", &["sat", p, C]);
    assert_eq!(capped.stdout.trim(), "unknown", "stderr: {}", capped.stderr);
    assert_eq!(capped.code, 3);
    assert!(
        capped.stderr.contains("RUSTDL_QUERY_MAX_NODES"),
        "{}",
        capped.stderr
    );
    let control = run("0", &["sat", p, C]);
    assert_eq!(control.stdout.trim(), "sat", "stderr: {}", control.stderr);
    assert_eq!(control.code, 0);
}

#[test]
fn subclass_prints_unknown_when_the_probe_hits_the_node_cap() {
    let f = fixture_named("subclass");
    let p = f.path().to_str().unwrap();
    let capped = run("1", &["subclass", p, C, D]);
    assert_eq!(capped.stdout.trim(), "unknown", "stderr: {}", capped.stderr);
    assert_eq!(capped.code, 3);
    let control = run("0", &["subclass", p, C, D]);
    assert_eq!(control.stdout.trim(), "no", "stderr: {}", control.stderr);
    assert_eq!(control.code, 0);
}

#[test]
fn explain_reports_unknown_when_the_probe_hits_the_node_cap() {
    let f = fixture_named("explain");
    let p = f.path().to_str().unwrap();
    let capped = run("1", &["explain", p, C, D]);
    assert!(capped.stdout.contains(": unknown"), "{}", capped.stdout);
    assert_eq!(capped.code, 3);
    let control = run("0", &["explain", p, C, D]);
    assert!(control.stdout.contains(": no"), "{}", control.stdout);
    assert_eq!(control.code, 0);
}

#[test]
fn instance_prints_unknown_when_the_probe_is_cut() {
    let f = fixture_named("instance");
    let p = f.path().to_str().unwrap();
    let capped = run("1", &["instance", p, D, A]);
    assert_eq!(capped.stdout.trim(), "unknown", "stderr: {}", capped.stderr);
    assert_eq!(capped.code, 3);
    let control = run("0", &["instance", p, D, A]);
    assert_eq!(control.stdout.trim(), "no", "stderr: {}", control.stderr);
    assert_eq!(control.code, 0);
}

#[test]
fn instances_and_realize_warn_when_a_probe_is_cut() {
    let f = fixture_named("lists");
    let p = f.path().to_str().unwrap();
    for args in [vec!["instances", p, D], vec!["realize", p]] {
        let capped = run("1", &args);
        assert_eq!(capped.code, 0, "{args:?}: {}", capped.stderr);
        assert!(
            capped.stderr.contains("probe was cut"),
            "{args:?} must warn on a cut probe; stderr: {}",
            capped.stderr
        );
        let control = run("0", &args);
        assert!(
            !control.stderr.contains("probe was cut"),
            "{args:?} must not warn without a cut; stderr: {}",
            control.stderr
        );
    }
}

/// With the per-probe deadline disabled, realize's instance probe takes the
/// deadline-free branch, where the node cap is the only give-up. That branch
/// must report the cut too.
#[test]
fn instance_with_no_probe_deadline_still_reports_the_cut() {
    let f = fixture_named("nodeadline");
    let p = f.path().to_str().unwrap();
    let no_deadline = [("RUSTDL_REALIZE_PAIR_TIMEOUT_MS", "0")];
    let capped = run_with("1", &no_deadline, &["instance", p, D, A]);
    assert_eq!(capped.stdout.trim(), "unknown", "stderr: {}", capped.stderr);
    assert_eq!(capped.code, 3);
    let control = run_with("0", &no_deadline, &["instance", p, D, A]);
    assert_eq!(control.stdout.trim(), "no", "stderr: {}", control.stderr);
    assert_eq!(control.code, 0);
}

/// #344: the reproducer is inconsistent, but only once the main tableau counts
/// its data values. With a zero verification budget that check gives up, and
/// `consistent` must say `unknown` (exit 3), not `consistent` (#363 A2). With the
/// default budget it decides `inconsistent`.
#[test]
fn consistent_reports_a_data_counting_verification_give_up() {
    const SRC_344: &str = r#"Prefix(:=<http://ex.org/>)
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)
Ontology(<http://ex.org/o>
  Declaration(Class(:P)) Declaration(DataProperty(:p)) Declaration(NamedIndividual(:a))
  SubClassOf(:P DataMaxCardinality(1 :p DataOneOf("1"^^xsd:integer "2"^^xsd:integer)))
  ClassAssertion(:P :a)
  DataPropertyAssertion(:p :a "1"^^xsd:integer)
  DataPropertyAssertion(:p :a "2"^^xsd:integer)
)
"#;
    let path = std::env::temp_dir().join(format!(
        "rustdl-query-give-up-{}-344.ofn",
        std::process::id()
    ));
    std::fs::write(&path, SRC_344).unwrap();
    let f = Fixture(path);
    let p = f.path().to_str().unwrap();
    let zero = [("RUSTDL_CONSISTENCY_DATA_COUNTING_VERIFY_MS", "0")];
    let gave_up = run_with("0", &zero, &["consistent", p]);
    assert_eq!(
        gave_up.stdout.trim(),
        "unknown",
        "stderr: {}",
        gave_up.stderr
    );
    assert_eq!(gave_up.code, 3);
    assert!(
        gave_up
            .stderr
            .contains("RUSTDL_CONSISTENCY_DATA_COUNTING_VERIFY_MS"),
        "{}",
        gave_up.stderr
    );
    let json = run_with("0", &zero, &["consistent", "--json", p]);
    assert_eq!(json.code, 3);
    assert!(
        json.stdout.contains("\"incomplete\": true"),
        "{}",
        json.stdout
    );
    let decided = run("0", &["consistent", p]);
    assert_eq!(
        decided.stdout.trim(),
        "inconsistent",
        "stderr: {}",
        decided.stderr
    );
    assert_eq!(decided.code, 0);
}

/// The deadline-free single-query cap (`RUSTDL_QUERY_MAX_NODES`, default 500000)
/// must reach the tableau search, which runs on its own large-stack thread. With
/// `RUSTDL_MAX_NODES` unset, a query cap of 1 must cut `sat` and a query cap of
/// 0 (disabled) must let it decide.
#[test]
fn the_single_query_cap_reaches_the_search_thread() {
    let f = fixture_named("querycap");
    let p = f.path().to_str().unwrap();
    let run_query_cap = |cap: &str| {
        let out = Command::new(env!("CARGO_BIN_EXE_rustdl"))
            .args(["sat", p, C])
            .env_remove("RUSTDL_MAX_NODES")
            .env("RUSTDL_QUERY_MAX_NODES", cap)
            .output()
            .unwrap();
        (
            out.status.code().unwrap_or(-1),
            String::from_utf8(out.stdout).unwrap().trim().to_owned(),
        )
    };
    assert_eq!(run_query_cap("1"), (3, "unknown".to_owned()));
    assert_eq!(run_query_cap("0"), (0, "sat".to_owned()));
}
