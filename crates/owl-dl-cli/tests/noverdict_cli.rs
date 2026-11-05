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
//!
//! #462: every other command keeps its own output format too. A `NoVerdict` on a probe
//! with NO deadline sets the command's give-up slot plus its own `no_verdict` /
//! `no_verdict_pairs` field, and the command adds #449's note and exits 3. A
//! `NoVerdict` on a probe WITH a deadline is the routine decision-depth cut (#288) and
//! changes nothing, so the deadline-free arms run with `--pair-timeout-ms 0`.
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

/// `SRC` with two individuals, so the `ABox` commands probe pairs.
const SRC_ABOX: &str = r"Prefix(:=<http://ex/#>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Ontology(<http://ex/noverdict-abox>
  Declaration(Class(:C)) Declaration(Class(:D)) Declaration(Class(:E))
  Declaration(ObjectProperty(:r)) Declaration(ObjectProperty(:s))
  Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))
  SubClassOf(:C ObjectSomeValuesFrom(:r :C))
  SubClassOf(:D ObjectSomeValuesFrom(:r :D))
  SubClassOf(:C ObjectAllValuesFrom(:s :E))
  SubClassOf(owl:Thing :E)
  ClassAssertion(:C :a) ClassAssertion(:D :b) ObjectPropertyAssertion(:r :a :b)
)
";

const C: &str = "http://ex/#C";
const D: &str = "http://ex/#D";
const MARKER: &str = "likely a reasoner bug";

struct Fixture(PathBuf);

impl Fixture {
    fn new(tag: &str) -> Self {
        Self::with(tag, SRC)
    }

    fn with(tag: &str, src: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("rustdl-noverdict-{}-{tag}.ofn", std::process::id()));
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
    assert_eq!(j["no_verdict"], true);
    // Control.
    let r = run(false, &["consistent", "--json", f.path()]);
    assert_eq!(r.code, 0, "{}", r.stderr);
    let j: serde_json::Value = serde_json::from_str(&r.stdout).unwrap();
    assert_eq!(j["consistent"], true);
    assert_eq!(j["incomplete"], false);
    assert_eq!(j["no_verdict"], false);
}

/// The JSON object, which must parse whatever the exit code.
fn json(r: &Run, what: &str) -> serde_json::Value {
    serde_json::from_str(&r.stdout)
        .unwrap_or_else(|e| panic!("{what}: stdout is JSON ({e})\nstdout: {}", r.stdout))
}

/// FORCED: JSON with `incomplete` and `no_verdict` (or `no_verdict_pairs > 0`), #449's
/// note, exit 3. Control: JSON, the field clear, exit 0. `flag` is the field's name.
fn assert_json_flags_no_verdict(f: &Fixture, args: &[&str], flag: &str) {
    let mut full: Vec<&str> = args.to_vec();
    full.push(f.path());
    let what = args.join(" ");
    let r = run(true, &full);
    assert_gave_up(&r, &what);
    let j = json(&r, &what);
    assert_eq!(j["incomplete"], true, "{what}: {j}");
    match flag {
        "no_verdict_pairs" => assert!(j[flag].as_u64().unwrap() > 0, "{what}: {j}"),
        _ => assert_eq!(j[flag], true, "{what}: {j}"),
    }
    let r = run(false, &full);
    assert_eq!(r.code, 0, "{what} control: {}", r.stderr);
    assert!(!r.stderr.contains(MARKER), "{what} control: {}", r.stderr);
    let j = json(&r, &what);
    match flag {
        "no_verdict_pairs" => assert_eq!(j[flag], 0, "{what} control: {j}"),
        _ => assert_eq!(j[flag], false, "{what} control: {j}"),
    }
}

#[test]
fn query_json_keeps_its_object() {
    let f = Fixture::new("queries");
    for (args, status) in [
        (vec!["prove", "--json", "PATH", C, D], Some("unknown")),
        (
            vec!["justify", "--json", "PATH", "subclass", C, D],
            Some("unknown"),
        ),
        (vec!["sat-expr", "--json", "PATH", "C"], None),
        (vec!["subclass-expr", "--json", "PATH", "C", "D"], None),
    ] {
        let full: Vec<&str> = args
            .iter()
            .map(|a| if *a == "PATH" { f.path() } else { a })
            .collect();
        let what = args[0];
        let r = run(true, &full);
        assert_gave_up(&r, what);
        let j = json(&r, what);
        assert_eq!(
            (j["incomplete"].clone(), j["no_verdict"].clone()),
            (true.into(), true.into()),
            "{what}: {j}"
        );
        if let Some(s) = status {
            assert_eq!(j["status"], s, "{what}: {j}");
        }
        let r = run(false, &full);
        assert_eq!(r.code, 0, "{what} control: {}", r.stderr);
        let j = json(&r, what);
        assert_eq!(j["no_verdict"], false, "{what} control: {j}");
        assert_eq!(j["incomplete"], false, "{what} control: {j}");
    }
}

#[test]
fn hierarchy_json_with_no_pair_budget_keeps_its_object() {
    let f = Fixture::new("hierarchy");
    assert_json_flags_no_verdict(
        &f,
        &["classify", "--json", "--pair-timeout-ms", "0"],
        "no_verdict_pairs",
    );
    assert_json_flags_no_verdict(
        &f,
        &[
            "classify",
            "--n2-classify",
            "--json",
            "--pair-timeout-ms",
            "0",
        ],
        "no_verdict_pairs",
    );
    assert_json_flags_no_verdict(
        &f,
        &["disjoint", "--json", "--pair-timeout-ms", "0"],
        "no_verdict",
    );
    // Not reported as a decision-depth cut: no budget cut these probes. And every cut
    // is counted as a NoVerdict: with no deadline nothing else cuts a probe here, so a
    // cut left out of `no_verdict_pairs` (a mapping site that drops the flag, or a
    // worker-stats merge that drops the field) shows up as a "class pair(s) hit the
    // ..." timeout line. `> 0` alone would not see it: both drivers make several cuts.
    for driver in [&[][..], &["--n2-classify"][..]] {
        let mut args = vec!["classify", "--json", "--pair-timeout-ms", "0"];
        args.extend_from_slice(driver);
        args.push(f.path());
        let r = run(true, &args);
        assert!(
            r.stderr.contains("without a verdict (NoVerdict)"),
            "{args:?}: {}",
            r.stderr
        );
        assert!(
            !r.stderr.contains("search-depth limit"),
            "{args:?}: {}",
            r.stderr
        );
        assert!(
            !r.stderr.contains("class pair(s) hit the"),
            "{args:?}: a cut was not counted in no_verdict_pairs: {}",
            r.stderr
        );
    }
}

#[test]
fn abox_json_with_no_pair_budget_keeps_its_object() {
    let f = Fixture::with("abox", SRC_ABOX);
    assert_json_flags_no_verdict(
        &f,
        &["individuals", "--json", "--pair-timeout-ms", "0"],
        "no_verdict",
    );
    assert_json_flags_no_verdict(
        &f,
        &["property-values", "--json", "--pair-timeout-ms", "0"],
        "no_verdict",
    );
}

#[test]
fn text_commands_give_up_in_their_own_format() {
    let f = Fixture::new("text");
    for args in [
        vec!["justify", f.path(), "subclass", C, D],
        vec!["repair", f.path(), "subclass", C, D],
        vec!["diagnose", f.path()],
        vec!["report", f.path()],
    ] {
        let what = args[0];
        let r = run(true, &args);
        assert_gave_up(&r, what);
        assert_ne!(
            r.stdout.trim(),
            "unknown",
            "{what}: its own format, not the bare token"
        );
        // Never blamed on the node cap alone (`diagnose` names both causes, since it
        // counts cut unsat probes together).
        assert!(
            r.stderr
                .lines()
                .filter(|l| l.contains("RUSTDL_MAX_NODES"))
                .all(|l| l.contains("NoVerdict")),
            "{what}: not blamed on the node cap: {}",
            r.stderr
        );
        let r = run(false, &args);
        assert_eq!(r.code, 0, "{what} control: {}", r.stderr);
        assert!(!r.stderr.contains(MARKER), "{what} control: {}", r.stderr);
    }
}

/// #462 option (i): with a pair budget, a `NoVerdict` is the routine decision-depth cut
/// (#288). The forced stall there is reported as a cut, as before: `incomplete` only,
/// exit 0, no note.
#[test]
fn budgeted_runs_are_unchanged() {
    let f = Fixture::new("budgeted");
    let a = Fixture::with("budgeted-abox", SRC_ABOX);
    for (args, flag) in [
        (vec!["classify", "--json", f.path()], "no_verdict_pairs"),
        (
            vec!["classify", "--n2-classify", "--json", f.path()],
            "no_verdict_pairs",
        ),
        (vec!["disjoint", "--json", f.path()], "no_verdict"),
        (vec!["individuals", "--json", a.path()], "no_verdict"),
        (vec!["property-values", "--json", a.path()], "no_verdict"),
    ] {
        let what = args[..args.len() - 1].join(" ");
        let r = run(true, &args);
        assert_eq!(r.code, 0, "{what}: {}", r.stderr);
        assert!(!r.stderr.contains(MARKER), "{what}: {}", r.stderr);
        let j = json(&r, &what);
        assert_eq!(j["incomplete"], true, "{what}: {j}");
        match flag {
            "no_verdict_pairs" => assert_eq!(j[flag], 0, "{what}: {j}"),
            _ => assert_eq!(j[flag], false, "{what}: {j}"),
        }
    }
}

/// #462: a real decision-depth cut (#288's fixture: `X`'s unsat probe needs 280 levels,
/// past the limit of 256) at a budget, unforced: exit 0, no note, `no_verdict_pairs: 0`,
/// and the depth-limit banner line. A `NoVerdict` on a budgeted probe must never read as
/// a reasoner bug (PMD core and several ORE ontologies hit this at the default).
#[test]
fn a_budgeted_depth_cut_is_not_a_no_verdict() {
    let body: Vec<String> = (0..280)
        .map(|k| format!("ObjectUnionOf(:A{k} :B{k})"))
        .collect();
    let src = format!(
        "Prefix(:=<http://example.org/d#>)\nOntology(<http://example.org/d>\n\
         SubClassOf(:X ObjectIntersectionOf({}))\n)\n",
        body.join(" ")
    );
    let f = Fixture::with("depth-cut", &src);
    let r = run(
        false,
        &["classify", "--json", "--pair-timeout-ms", "20000", f.path()],
    );
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert!(!r.stderr.contains(MARKER), "{}", r.stderr);
    assert!(
        r.stderr.contains("search-depth limit"),
        "the cut happened: {}",
        r.stderr
    );
    let j = json(&r, "classify");
    assert_eq!(j["incomplete"], true, "{j}");
    assert_eq!(j["no_verdict_pairs"], 0, "{j}");
}
