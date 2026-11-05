//! #449: a tableau `NoVerdict` (an internal limit) is a give-up on the CLI: `unknown`,
//! a "likely a reasoner bug" stderr line, exit 3. It used to be exit 1 with an error chain
//! and no output.
//!
//! No input in the implemented fragment is known to reach `NoVerdict` since #437, so the
//! tests force it: `RUSTDL_TABLEAU_SATURATE_ITERS=1` lets the deterministic saturation
//! run one iteration, and any root that needs a rule application then ends `Stalled`,
//! which a deadline-free probe reports as `NoVerdict`. Every test has a control arm
//! without the knob that must get the definite answer, so a fixture that gave up at the
//! default could not pass by pinning a live bug.
//!
//! `sat` and `subclass` reach the top-level net in `main`; `explain` and
//! `consistent --json` handle it at their own sites, to keep their output formats.
#![allow(clippy::unwrap_used)]

use std::path::PathBuf;
use std::process::Command;

/// Satisfiable; `C` and `D` are unrelated. The `∀s.E` keeps it out of the EL fragment,
/// so the saturation fast path cannot answer. `⊤ ⊑ E` gives the consistency check's root
/// a rule to apply. No individuals, so `consistent` runs the main tableau's
/// deadline-free check rather than the `ABox`-seeded wedge.
const SRC: &str = r"Prefix(:=<http://ex/#>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Ontology(<http://ex/noverdict>
  Declaration(Class(:C)) Declaration(Class(:D)) Declaration(Class(:E))
  Declaration(ObjectProperty(:r)) Declaration(ObjectProperty(:s))
  SubClassOf(:C ObjectSomeValuesFrom(:r :C))
  SubClassOf(:D ObjectSomeValuesFrom(:r :D))
  SubClassOf(:C ObjectAllValuesFrom(:s :E))
  SubClassOf(owl:Thing :E)
)
";

const C: &str = "http://ex/#C";
const D: &str = "http://ex/#D";
const MARKER: &str = "likely a reasoner bug";

struct Fixture(PathBuf);

impl Fixture {
    fn new(tag: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("rustdl-noverdict-{}-{tag}.ofn", std::process::id()));
        std::fs::write(&path, SRC).unwrap();
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

/// Runs the binary with no inherited `RUSTDL_*` settings, the wedge off (so the main
/// tableau decides), and, when `forced`, a one-iteration saturation cap.
fn run(forced: bool, args: &[&str]) -> Run {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_rustdl"));
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("RUSTDL_") {
            cmd.env_remove(key);
        }
    }
    cmd.env("RUSTDL_HYPERTABLEAU", "0")
        .env("RUSTDL_WEDGE_CONSISTENCY", "0");
    if forced {
        cmd.env("RUSTDL_TABLEAU_SATURATE_ITERS", "1");
    }
    let out = cmd.args(args).output().unwrap();
    Run {
        code: out.status.code().unwrap_or(-1),
        stdout: String::from_utf8(out.stdout).unwrap(),
        stderr: String::from_utf8(out.stderr).unwrap(),
    }
}

fn assert_gave_up(r: &Run, what: &str) {
    assert_eq!(
        r.code, 3,
        "{what}: exit 3\nstdout: {}\nstderr: {}",
        r.stdout, r.stderr
    );
    assert!(
        r.stderr.contains(MARKER),
        "{what}: stderr names the bug: {}",
        r.stderr
    );
}

#[test]
fn sat_and_subclass_report_unknown() {
    let f = Fixture::new("net");
    let sat = run(true, &["sat", f.path(), C]);
    assert_gave_up(&sat, "sat");
    assert_eq!(sat.stdout.trim(), "unknown");
    let sub = run(true, &["subclass", f.path(), C, D]);
    assert_gave_up(&sub, "subclass");
    assert_eq!(sub.stdout.trim(), "unknown");
    // Control: without the knob both are definite.
    let sat = run(false, &["sat", f.path(), C]);
    assert_eq!((sat.code, sat.stdout.trim()), (0, "sat"), "{}", sat.stderr);
    let sub = run(false, &["subclass", f.path(), C, D]);
    assert_eq!((sub.code, sub.stdout.trim()), (0, "no"), "{}", sub.stderr);
}

#[test]
fn explain_keeps_its_format() {
    let f = Fixture::new("explain");
    let r = run(true, &["explain", f.path(), C, D]);
    assert_gave_up(&r, "explain");
    assert!(
        r.stdout
            .contains(": unknown — the tableau reached an internal limit"),
        "{}",
        r.stdout
    );
    let r = run(false, &["explain", f.path(), C, D]);
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert!(!r.stdout.contains("unknown"), "{}", r.stdout);
}

#[test]
fn consistent_reports_unknown_and_json_reports_incomplete() {
    let f = Fixture::new("consistent");
    let r = run(true, &["consistent", f.path()]);
    assert_gave_up(&r, "consistent");
    assert_eq!(r.stdout.trim(), "unknown");
    let r = run(true, &["consistent", "--json", f.path()]);
    assert_gave_up(&r, "consistent --json");
    let j: serde_json::Value = serde_json::from_str(&r.stdout).unwrap();
    assert_eq!(j["consistent"], true);
    assert_eq!(j["incomplete"], true);
    // Control.
    let r = run(false, &["consistent", "--json", f.path()]);
    assert_eq!(r.code, 0, "{}", r.stderr);
    let j: serde_json::Value = serde_json::from_str(&r.stdout).unwrap();
    assert_eq!(j["consistent"], true);
    assert_eq!(j["incomplete"], false);
}
