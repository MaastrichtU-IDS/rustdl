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
//! `no_verdict_pairs` field, and the command adds #449's note and exits 3.
//!
//! #475: on a probe WITH a deadline the same holds when its saturation stalled (the
//! forced stall here). A budgeted `NoVerdict` that is the routine decision-depth cut
//! (#288) still changes nothing: `a_budgeted_depth_cut_is_not_a_no_verdict` guards it.
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
    run_with(forced, &[], args)
}

/// [`run`] with extra `RUSTDL_*` settings, applied last so they override its defaults.
fn run_with(forced: bool, env: &[(&str, &str)], args: &[&str]) -> Run {
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
    for (k, v) in env {
        cmd.env(k, v);
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

/// #475: with a pair budget, a probe whose saturation STALLED is the #462 signal too:
/// the command's `no_verdict` / `no_verdict_pairs` field, #449's note and exit 3, as on
/// a deadline-free probe. (Before #475 a budgeted `NoVerdict` could not tell a stall
/// from the routine decision-depth cut, so it read as a cut.) The budgets are generous
/// so that a loaded machine cannot turn the forced stall into a plain deadline cut, and
/// the absence of the depth-limit line shows the stall, not a cut, was reported. The
/// control arm, unforced at the same budget, gets a definite answer and no signal.
#[test]
fn a_budgeted_stall_is_a_no_verdict() {
    let f = Fixture::new("budgeted");
    let a = Fixture::with("budgeted-abox", SRC_ABOX);
    let pt = ["--pair-timeout-ms", "20000"];
    for (cmd, file, flag) in [
        (&["classify", "--json"][..], f.path(), "no_verdict_pairs"),
        (
            &["classify", "--n2-classify", "--json"][..],
            f.path(),
            "no_verdict_pairs",
        ),
        (&["disjoint", "--json"][..], f.path(), "no_verdict"),
        (&["individuals", "--json"][..], a.path(), "no_verdict"),
        (&["property-values", "--json"][..], a.path(), "no_verdict"),
    ] {
        let mut args: Vec<&str> = cmd.to_vec();
        args.extend_from_slice(&pt);
        args.push(file);
        let what = cmd.join(" ");
        let r = run(true, &args);
        assert_gave_up(&r, &what);
        assert!(
            !r.stderr.contains("search-depth limit"),
            "{what}: a stall is not reported as a depth cut: {}",
            r.stderr
        );
        let j = json(&r, &what);
        assert_eq!(j["incomplete"], true, "{what}: {j}");
        match flag {
            "no_verdict_pairs" => {
                assert!(j[flag].as_u64().unwrap() > 0, "{what}: {j}");
            }
            _ => assert_eq!(j[flag], true, "{what}: {j}"),
        }
        let r = run(false, &args);
        assert_eq!(r.code, 0, "{what} control: {}", r.stderr);
        assert!(!r.stderr.contains(MARKER), "{what} control: {}", r.stderr);
        let j = json(&r, &what);
        match flag {
            "no_verdict_pairs" => assert_eq!(j[flag], 0, "{what} control: {j}"),
            _ => assert_eq!(j[flag], false, "{what} control: {j}"),
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

/// The 280-union class `X` with an individual in it. With the consistency wedge on,
/// the wedge stalls on it and `consistent` falls through to its BOUNDED tableau probe
/// (`RUSTDL_CONSISTENCY_FALLBACK_MS`).
fn unions_abox() -> String {
    let body: Vec<String> = (0..280)
        .map(|k| format!("ObjectUnionOf(:A{k} :B{k})"))
        .collect();
    format!(
        "Prefix(:=<http://example.org/d#>)\nOntology(<http://example.org/d>\n\
         Declaration(NamedIndividual(:a))\n\
         SubClassOf(:X ObjectIntersectionOf({}))\nClassAssertion(:X :a)\n)\n",
        body.join(" ")
    )
}

/// #475: the consistency check's wedge-route fall-through probe has a deadline, so
/// #462 left its `NoVerdict` a give-up (Fable's review of #471). A probe whose
/// saturation STALLED is the #462 signal even there: `consistent --json` reports
/// `no_verdict: true` with the note and exit 3. The fall-through budget is generous, so
/// a loaded machine cannot make the forced stall a plain deadline give-up.
#[test]
fn a_budgeted_consistency_stall_is_a_no_verdict() {
    let f = Fixture::with("consistency-fallthrough", &unions_abox());
    let on = [
        ("RUSTDL_WEDGE_CONSISTENCY", "1"),
        ("RUSTDL_TRACE", "1"),
        ("RUSTDL_CONSISTENCY_FALLBACK_MS", "60000"),
    ];
    let r = run_with(true, &on, &["consistent", "--json", f.path()]);
    // The route: the wedge stalled and the bounded fall-through ran.
    assert!(
        r.stderr.contains("bounded tableau fall-through"),
        "the bounded fall-through ran: {}",
        r.stderr
    );
    // #475 (Fable's review): the stall is reported only after the ABox-components step
    // had its chance (a witnessed inconsistency would win), so its trace line prints.
    assert!(
        r.stderr.contains("gave up (saturation stalled")
            && r.stderr.contains("trying ABox components"),
        "the stall reached the components step: {}",
        r.stderr
    );
    assert_gave_up(&r, "consistent, budgeted fall-through");
    let j = json(&r, "consistent");
    assert_eq!(j["no_verdict"], true, "{j}");
    assert_eq!(j["incomplete"], true, "{j}");
    // The deadline-free path (wedge route off) on the same input reports it as before.
    let r = run(true, &["consistent", "--json", f.path()]);
    assert_gave_up(&r, "consistent, deadline-free");
    assert_eq!(json(&r, "consistent")["no_verdict"], true);
}

/// #462: a deadline-free `NoVerdict` cut is marked even when
/// `RUSTDL_UNSAT_PROBE_CUT_INCOMPLETE=0` stops recording other cut unsat probes, so
/// `no_verdict_pairs` never comes with `incomplete: false` (Fable's review of #471).
#[test]
fn a_no_verdict_cut_is_incomplete_whatever_the_cut_flag_says() {
    let f = Fixture::new("cut-flag-off");
    let off = [("RUSTDL_UNSAT_PROBE_CUT_INCOMPLETE", "0")];
    for driver in [&[][..], &["--n2-classify"][..]] {
        let mut args = vec!["classify", "--json", "--pair-timeout-ms", "0"];
        args.extend_from_slice(driver);
        args.push(f.path());
        let r = run_with(true, &off, &args);
        let j = json(&r, "classify");
        assert!(j["no_verdict_pairs"].as_u64().unwrap() > 0, "{args:?}: {j}");
        assert_eq!(j["incomplete"], true, "{args:?}: {j}");
        // The cut unsat probes count in `cut_unsat_probes` (#470), which admits the
        // consistency probe on this TBox-only input; under the forced stall it gives up.
        // An unmarked cut would leave classes undecided with consistency claimed
        // determined (#466's shape). `incomplete` alone cannot see it here: the pair
        // probes' own `NoVerdict` cuts set it anyway.
        assert_eq!(j["consistency_undetermined"], true, "{args:?}: {j}");
        assert!(
            !r.stderr.contains("class pair(s) hit the"),
            "{args:?}: {}",
            r.stderr
        );
    }
}
