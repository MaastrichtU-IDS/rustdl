//! #392: `consistent` reports a verdict that rests only on the consistency
//! wedge's `Sat` (nothing verified it): `relies_on_trusted_consistency` in
//! `--json`, and one stderr note in both modes. Not a give-up: stdout stays
//! `consistent`, `incomplete` stays false and the exit code stays 0.
#![allow(clippy::unwrap_used)]

use std::path::PathBuf;
use std::process::Command;

/// Off pure EL (the union), with an `ABox`, no nominal and no data counting:
/// the wedge decides it `Sat` (#401's fixture).
const ONTO: &str = r"Prefix(:=<http://ex/#>)
Ontology(<http://ex/trustedsat>
  Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C)) Declaration(Class(:D))
  Declaration(ObjectProperty(:r))
  Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))
  SubClassOf(:A ObjectUnionOf(:C :D))
  SubClassOf(:C :B) SubClassOf(:D :B)
  ClassAssertion(:A :a) ObjectPropertyAssertion(:r :a :b)
  EXTRA
)
";

const NOTE: &str = "note: consistent on the consistency wedge's model alone";

/// The whole note, as one line.
/// #497: with the wedge off the main tableau has NO deadline (it used to claim a
/// `RUSTDL_CONSISTENCY_FALLBACK_MS` bound, which applies only to the wedge route).
const FULL_NOTE: &str = "note: consistent on the consistency wedge's model alone; the wedge is \
                         not proven complete and no main-tableau model was built. \
                         RUSTDL_WEDGE_CONSISTENCY=0 decides on the main tableau instead, with \
                         no deadline; on a hard ontology it may not finish (wine does not, \
                         #490).\n";

/// Writes the fixture to a per-test file (tests run in parallel).
struct Fixture(PathBuf);

impl Fixture {
    fn new(tag: &str, extra: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "rustdl-consistent-trusted-sat-{}-{tag}.ofn",
            std::process::id()
        ));
        std::fs::write(&path, ONTO.replace("EXTRA", extra)).unwrap();
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

fn json(r: &Run) -> serde_json::Value {
    serde_json::from_str(&r.stdout).unwrap()
}

#[test]
fn a_wedge_sat_is_consistent_with_a_note() {
    let f = Fixture::new("text", "");
    let r = run(&["consistent", f.path()], &[]);
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert_eq!(r.stdout, "consistent\n");
    assert_eq!(r.stderr, FULL_NOTE);
}

#[test]
fn a_wedge_sat_is_flagged_in_json() {
    let f = Fixture::new("json", "");
    let r = run(&["consistent", "--json", f.path()], &[]);
    assert_eq!(r.code, 0, "{}", r.stderr);
    let v = json(&r);
    assert_eq!(v["consistent"], true, "{}", r.stdout);
    assert_eq!(v["relies_on_trusted_consistency"], true, "{}", r.stdout);
    assert_eq!(v["incomplete"], false, "a trust decision, not a give-up");
    assert!(r.stderr.contains(NOTE), "{}", r.stderr);
}

#[test]
fn a_main_tableau_model_is_not_flagged() {
    let f = Fixture::new("off", "");
    let off = [("RUSTDL_WEDGE_CONSISTENCY", "0")];
    let text = run(&["consistent", f.path()], &off);
    assert_eq!(text.code, 0, "{}", text.stderr);
    assert_eq!(text.stdout, "consistent\n");
    assert!(!text.stderr.contains(NOTE), "{}", text.stderr);
    let r = run(&["consistent", "--json", f.path()], &off);
    let v = json(&r);
    assert_eq!(v["consistent"], true, "{}", r.stdout);
    assert_eq!(v["relies_on_trusted_consistency"], false, "{}", r.stdout);
}

#[test]
fn an_inconsistency_is_not_flagged() {
    let f = Fixture::new("incons", "ClassAssertion(ObjectComplementOf(:B) :a)");
    let r = run(&["consistent", "--json", f.path()], &[]);
    assert_eq!(r.code, 0, "{}", r.stderr);
    let v = json(&r);
    assert_eq!(v["consistent"], false, "{}", r.stdout);
    assert_eq!(v["relies_on_trusted_consistency"], false, "{}", r.stdout);
    assert!(!r.stderr.contains(NOTE), "{}", r.stderr);
}

/// With refused clauses too (#357), both lines print: the trust note first, then
/// the narrower refused-clauses warning. `RUSTDL_WEDGE_BODY_DECOMPOSE=0` leaves
/// the 9-variable star body refused (`wedge_body_decompose.rs`).
#[test]
fn the_note_precedes_the_refused_clauses_warning() {
    use std::fmt::Write as _;
    let mut star = String::from(
        "SubClassOf(:B1 :J) SubClassOf(:B2 :J) DisjointClasses(:B1 :B2)\n\
         SubClassOf(:A ObjectSomeValuesFrom(:q :B1)) SubClassOf(:A ObjectSomeValuesFrom(:q :B2))\n",
    );
    let mut lhs = String::new();
    for i in 1..=8 {
        writeln!(star, "SubClassOf(:A ObjectSomeValuesFrom(:s{i} :S{i}))").unwrap();
        write!(lhs, " ObjectSomeValuesFrom(:s{i} :S{i})").unwrap();
    }
    writeln!(
        star,
        "SubClassOf(ObjectIntersectionOf({lhs} ObjectMinCardinality(2 :q :J)) :G)"
    )
    .unwrap();
    let f = Fixture::new("refused", &star);
    let r = run(
        &["consistent", "--json", f.path()],
        &[("RUSTDL_WEDGE_BODY_DECOMPOSE", "0")],
    );
    assert_eq!(r.code, 0, "{}", r.stderr);
    let v = json(&r);
    assert_eq!(v["relies_on_trusted_consistency"], true, "{}", r.stdout);
    assert_eq!(v["wedge_clauses_refused"], 1, "{}", r.stdout);
    let note = r.stderr.find(NOTE).expect("note");
    let refused = r.stderr.find("NOT GUARANTEED COMPLETE").expect("warning");
    assert!(note < refused, "{}", r.stderr);
}

/// #497: `sat owl:Thing` is the consistency check (#427) and prints the same note.
#[test]
fn sat_owl_thing_prints_the_same_note() {
    let f = Fixture::new("sat-thing", "");
    let r = run(
        &["sat", f.path(), "http://www.w3.org/2002/07/owl#Thing"],
        &[],
    );
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert_eq!(r.stdout.trim(), "sat", "{}", r.stdout);
    assert!(r.stderr.contains(FULL_NOTE), "{}", r.stderr);
}

/// #497: on an ontology with no individual the consistency check runs the main tableau
/// with no deadline, so its only give-up is the node cap. The advice must name it (it
/// used to say only "raise `RUSTDL_CONSISTENCY_FALLBACK_MS`", which bounds nothing there).
/// Off EL (the union), so the saturator does not decide it; `⊤ ⊑ ∃r.C` needs a node.
#[test]
fn a_node_cap_give_up_without_the_wedge_names_the_node_cap() {
    /// Removes the fixture even when an assertion fails.
    struct TempOnto(PathBuf);
    impl Drop for TempOnto {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }
    let f = TempOnto(std::env::temp_dir().join(format!(
        "rustdl-consistent-trusted-sat-{}-nocap.ofn",
        std::process::id()
    )));
    std::fs::write(
        &f.0,
        "Prefix(:=<http://ex/#>)\nPrefix(owl:=<http://www.w3.org/2002/07/owl#>)\n\
         Ontology(<http://ex/nocap>\n\
         Declaration(Class(:C)) Declaration(Class(:D)) Declaration(Class(:E))\n\
         Declaration(ObjectProperty(:r))\n\
         SubClassOf(owl:Thing ObjectSomeValuesFrom(:r :C))\n\
         SubClassOf(:C ObjectUnionOf(:D :E))\n)\n",
    )
    .unwrap();
    let p = f.0.to_str().unwrap();
    for (args, cap_env, cap) in [
        (
            vec!["consistent", p],
            "RUSTDL_MAX_NODES",
            "raise RUSTDL_MAX_NODES",
        ),
        (
            vec!["sat", p, "http://www.w3.org/2002/07/owl#Thing"],
            "RUSTDL_QUERY_MAX_NODES",
            "raise RUSTDL_QUERY_MAX_NODES",
        ),
    ] {
        let what = args.join(" ");
        let r = run(&args, &[(cap_env, "1")]);
        assert_eq!(r.code, 3, "{what}: a give-up: {}\n{}", r.stdout, r.stderr);
        assert_eq!(r.stdout.trim(), "unknown", "{what}: {}", r.stdout);
        assert!(
            r.stderr.contains("the main tableau has no deadline") && r.stderr.contains(cap),
            "{what}: the advice names the node cap: {}",
            r.stderr
        );
    }
    // Negative control: `consistent` does not hold the single-query cap, so
    // `RUSTDL_QUERY_MAX_NODES` is not its cap (the advice names `RUSTDL_MAX_NODES`).
    let r = run(&["consistent", p], &[("RUSTDL_QUERY_MAX_NODES", "1")]);
    assert_eq!(r.code, 0, "{}\n{}", r.stdout, r.stderr);
    assert_eq!(r.stdout.trim(), "consistent", "{}", r.stdout);
}
