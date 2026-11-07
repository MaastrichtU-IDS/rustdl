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
//!
//! #493: `realize`, `instances-expr`, `instances` and `instance` follow the same rule for
//! their instance probes, #388's direct-types checks and (realize) the classification
//! under them; `a_budgeted_instance_depth_cut_is_not_a_no_verdict` guards the depth cut.
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

// ---------------------------------------------------------------------------------------
// #493: `realize`, `instances-expr`, `instances` and `instance` keep their output on a
// likely-bug `NoVerdict` and report it: `realize --json` / `instances-expr --json` get
// `no_verdict: true` (and `incomplete`), every command adds #449's note and exits 3.
// ---------------------------------------------------------------------------------------

/// A pure-EL `TBox`, so the realize classification answers by saturation and runs no
/// tableau pair (its control below checks that). `⊤ ⊑ E` gives every probe root a rule,
/// so the forced stall fires. `RUSTDL_REALIZE_SATURATION=0` sends realize down the
/// tableau path. `assertions` are the `ABox`.
fn el_abox(assertions: &str) -> String {
    format!(
        "Prefix(:=<http://ex/#>)\n\
         Prefix(owl:=<http://www.w3.org/2002/07/owl#>)\n\
         Ontology(<http://ex/el-abox>\n\
         Declaration(Class(:C)) Declaration(Class(:D)) Declaration(Class(:E))\n\
         Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))\n\
         SubClassOf(owl:Thing :E)\n\
         {assertions}\n)\n"
    )
}

/// The tableau realize path, a generous pair budget (so a forced stall can never be a
/// plain deadline cut on a loaded machine), and any extra settings.
const REALIZE_TABLEAU: [(&str, &str); 2] = [
    ("RUSTDL_REALIZE_SATURATION", "0"),
    ("RUSTDL_REALIZE_PAIR_TIMEOUT_MS", "20000"),
];

/// #493 control for the `el_abox` fixtures: the realize classification itself does not
/// stall under the forced knob (a pure-EL `TBox` classifies by saturation), so whatever sets
/// realize's `no_verdict` below comes from realize's own probes.
fn assert_el_classify_is_clean(f: &Fixture) {
    let r = run(true, &["classify", "--json", f.path()]);
    assert_eq!(r.code, 0, "classify on the EL fixture: {}", r.stderr);
    assert_eq!(json(&r, "classify")["no_verdict_pairs"], 0, "{}", r.stdout);
}

/// #493 (S1): an instance probe that stalls. `b : C` is told and `D` is not, so realize
/// runs `{b} ⊓ ¬D` (and `a`'s probes) on the main tableau. Budgeted (20 s) and
/// deadline-free (`RUSTDL_REALIZE_PAIR_TIMEOUT_MS=0`): either way the JSON is kept, with
/// `no_verdict` and `incomplete`, the note and exit 3. Unforced: a clean answer.
#[test]
fn realize_reports_a_stalled_instance_probe() {
    let f = Fixture::with("realize-probe", &el_abox("ClassAssertion(:C :b)"));
    assert_el_classify_is_clean(&f);
    for ms in ["20000", "0"] {
        let env = [
            ("RUSTDL_REALIZE_SATURATION", "0"),
            ("RUSTDL_REALIZE_PAIR_TIMEOUT_MS", ms),
        ];
        let what = format!("realize --json, pair budget {ms}");
        let r = run_with(true, &env, &["realize", "--json", f.path()]);
        assert_gave_up(&r, &what);
        let j = json(&r, &what);
        assert_eq!(j["no_verdict"], true, "{what}: {j}");
        assert_eq!(j["incomplete"], true, "{what}: {j}");
        // The JSON is the command's own (#493: a deadline-free `NoVerdict` used to fail
        // the whole command, so this object was lost).
        assert!(j["individuals"].is_array(), "{what}: {j}");
        let r = run_with(false, &env, &["realize", "--json", f.path()]);
        assert_eq!(r.code, 0, "{what} control: {}", r.stderr);
        assert!(!r.stderr.contains(MARKER), "{what} control: {}", r.stderr);
        assert_eq!(json(&r, &what)["no_verdict"], false, "{what} control");
    }
    // Text mode: the realization prints, then the note, exit 3; no budget advice.
    let r = run_with(true, &REALIZE_TABLEAU, &["realize", f.path()]);
    assert_gave_up(&r, "realize (text)");
    assert!(
        r.stdout.contains("http://ex/#b"),
        "realize (text) kept its output: {}",
        r.stdout
    );
    assert!(
        !r.stderr.contains("RUSTDL_MAX_NODES"),
        "realize (text): the note, not the budget advice: {}",
        r.stderr
    );
}

/// #493 (S3): #388's direct-types check. Both individuals are told `C` and `D` (every
/// instance check is a closure hit, so no instance probe runs), and `C`, `D` are
/// unrelated co-listed most-specific types, so realize checks `C ⊑ D` / `D ⊑ C` on the
/// main tableau, and the forced stall hits there.
#[test]
fn realize_reports_a_stalled_direct_types_check() {
    let f = Fixture::with(
        "realize-pair",
        &el_abox(
            "ClassAssertion(:C :a) ClassAssertion(:D :a) \
             ClassAssertion(:C :b) ClassAssertion(:D :b)",
        ),
    );
    assert_el_classify_is_clean(&f);
    let r = run_with(true, &REALIZE_TABLEAU, &["realize", "--json", f.path()]);
    assert_gave_up(&r, "realize --json, #388 check");
    let j = json(&r, "realize --json");
    assert_eq!(j["no_verdict"], true, "{j}");
    assert_eq!(
        j["direct_types_unverified"], true,
        "the cut pair stays unchecked: {j}"
    );
    let r = run_with(false, &REALIZE_TABLEAU, &["realize", "--json", f.path()]);
    assert_eq!(r.code, 0, "control: {}", r.stderr);
    let j = json(&r, "control");
    assert_eq!(j["no_verdict"], false, "control: {j}");
    assert_eq!(j["direct_types_unverified"], false, "control: {j}");
}

/// #493 (S6): realize's own classification. `SRC` has no individuals, so no instance
/// probe and no #388 pair runs; the classification under realize is budgeted and stalls
/// (#475), and realize reports it like `classify --json` does.
#[test]
fn realize_reports_a_stall_in_its_classification() {
    let f = Fixture::new("realize-seed");
    let r = run_with(true, &REALIZE_TABLEAU, &["realize", "--json", f.path()]);
    assert_gave_up(&r, "realize --json, no individuals");
    let j = json(&r, "realize --json");
    assert_eq!(j["no_verdict"], true, "{j}");
    assert_eq!(j["incomplete"], true, "{j}");
    assert_eq!(j["individuals"], serde_json::json!([]), "{j}");
}

/// #493: the instance queries share realize's probe. `instances-expr --json` reports the
/// field; `instances` keeps its list; `instance` answers `unknown` with #449's note (not
/// the probe-cut warning). Budgeted and deadline-free, with unforced controls.
#[test]
fn instance_queries_report_a_stalled_probe() {
    let a = Fixture::with("instances-abox", SRC_ABOX);
    for ms in ["20000", "0"] {
        let env = [("RUSTDL_REALIZE_PAIR_TIMEOUT_MS", ms)];
        let what = format!("instances-expr --json, pair budget {ms}");
        let r = run_with(true, &env, &["instances-expr", "--json", a.path(), ":D"]);
        assert_gave_up(&r, &what);
        let j = json(&r, &what);
        assert_eq!(j["no_verdict"], true, "{what}: {j}");
        assert_eq!(j["incomplete"], true, "{what}: {j}");
        let r = run_with(false, &env, &["instances-expr", "--json", a.path(), ":D"]);
        assert_eq!(r.code, 0, "{what} control: {}", r.stderr);
        assert_eq!(json(&r, &what)["no_verdict"], false, "{what} control");

        let what = format!("instances, pair budget {ms}");
        let r = run_with(true, &env, &["instances", a.path(), "http://ex/#D"]);
        assert_gave_up(&r, &what);
        assert!(
            !r.stderr.contains("RUSTDL_QUERY_MAX_NODES"),
            "{what}: {}",
            r.stderr
        );
        let r = run_with(false, &env, &["instances", a.path(), "http://ex/#D"]);
        assert_eq!(r.code, 0, "{what} control: {}", r.stderr);
        assert!(
            r.stdout.contains("http://ex/#b"),
            "{what} control: {}",
            r.stdout
        );

        let what = format!("instance, pair budget {ms}");
        let r = run_with(
            true,
            &env,
            &["instance", a.path(), "http://ex/#D", "http://ex/#a"],
        );
        assert_gave_up(&r, &what);
        assert_eq!(r.stdout.trim(), "unknown", "{what}: {}", r.stdout);
        assert!(
            !r.stderr.contains("instance probe was cut"),
            "{what}: {}",
            r.stderr
        );
        let r = run_with(
            false,
            &env,
            &["instance", a.path(), "http://ex/#D", "http://ex/#a"],
        );
        assert_eq!(r.code, 0, "{what} control: {}", r.stderr);
    }
}

/// #493 (S2): a budgeted instance probe that is the routine decision-depth cut (#288) is
/// NOT the signal. The fixture is the 280-union with every `Bk` disjoint from `X`, so each
/// `Bk` branch clashes at once and `{a} ⊓ ¬Y` (`Y` unrelated, so satisfiable) can only go
/// down the `Ak` choices: the search reaches the depth limit (256) in a line, every
/// alternative clashes, and it ends `DepthLimit` well before the 20 s deadline. Early
/// abandon is off, so this is `map_depth_limit`'s pure depth-cap arm (`Err(NoVerdict)` with
/// time left), the one a wrong flag would hit. (The plain 280-union never exhausts: with
/// early abandon off it runs into the deadline instead, which S2 cannot reach.) `instance`
/// reports the cut as before (`unknown`, the probe-cut warning, exit 3), with no note.
#[test]
fn a_budgeted_instance_depth_cut_is_not_a_no_verdict() {
    let body: Vec<String> = (0..280)
        .map(|k| format!("ObjectUnionOf(:A{k} :B{k})"))
        .collect();
    let disjoint: Vec<String> = (0..280)
        .map(|k| format!("DisjointClasses(:B{k} :X)"))
        .collect();
    let src = format!(
        "Prefix(:=<http://example.org/d#>)\nOntology(<http://example.org/d>\n\
         Declaration(NamedIndividual(:a)) Declaration(Class(:Y))\n\
         SubClassOf(:X ObjectIntersectionOf({}))\n{}\nClassAssertion(:X :a)\n)\n",
        body.join(" "),
        disjoint.join("\n")
    );
    let f = Fixture::with("instance-depthcut", &src);
    let env = [
        ("RUSTDL_TABLEAU_EARLY_ABANDON", "0"),
        ("RUSTDL_REALIZE_PAIR_TIMEOUT_MS", "20000"),
    ];
    let args = [
        "instance",
        f.path(),
        "http://example.org/d#Y",
        "http://example.org/d#a",
    ];
    let t = std::time::Instant::now();
    let r = run_with(false, &env, &args);
    // The search exhausts at the depth limit in about 0.5 s (debug); a run near the 20 s
    // budget would be a deadline cut (`Ok(None)`), which passes the checks below without
    // ever reaching the depth-cap arm this guards (Fable's review of #495).
    assert!(
        t.elapsed() < std::time::Duration::from_secs(10),
        "the probe must end on the depth limit, well before its deadline: {:?}",
        t.elapsed()
    );
    assert_eq!(
        r.code, 3,
        "the cut is a give-up: {}\n{}",
        r.stdout, r.stderr
    );
    assert_eq!(r.stdout.trim(), "unknown", "{}", r.stdout);
    assert!(r.stderr.contains("instance probe was cut"), "{}", r.stderr);
    assert!(
        !r.stderr.contains(MARKER),
        "a depth cut is not the bug signal: {}",
        r.stderr
    );
}

// ---------------------------------------------------------------------------------------
// #493 part 2: the #369 lazy consistency check, run after a POSITIVE answer, reports a
// stall of its own probe (the #344 verify or the main-tableau tail) as the #462 signal:
// the answer stands, `consistency_undetermined` and the result's `no_verdict` are set, and
// the command adds #449's note and exits 3. A depth cut there stays plain undetermined.
//
// The lazy check runs only after a positive answer, which a stalled probe cannot give, so
// these fixtures get their answer before the knob bites (a told fact, or a probe that
// clashes at iteration 2 while the lazy tail's root is still growing at `ITERS=4`). Each
// test asserts the lazy trace line first, so an answer from another route cannot pass
// vacuously.
// ---------------------------------------------------------------------------------------

fn lazy_fixture(name: &str) -> String {
    format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"))
}

/// The lazy check's trace line (#369), which the tests read as a precondition.
fn lazy_line(r: &Run) -> String {
    r.stderr
        .lines()
        .find(|l| l.starts_with("lazy consistency (#369):"))
        .unwrap_or_else(|| panic!("no lazy-check trace line: {}", r.stderr))
        .to_owned()
}

/// #493 (b): the prepared route's no-seed tail (#492's default). `chain8`: `⊤ ⊑ E` and an
/// 8-level `∃r` chain under `⊤`, so the tail's consistency probe is still growing at
/// iteration 4, while `U`, `X` (`∃s.⊤ ⊓ ∀s.¬E`) clash at iteration 2. The consistency
/// wedge is on (no seed, so the tail runs by default).
#[test]
fn a_stall_in_the_no_seed_lazy_tail_is_reported() {
    let f = lazy_fixture("lazy_tail_chain8_493.ofn");
    let env = [
        ("RUSTDL_TABLEAU_SATURATE_ITERS", "4"),
        ("RUSTDL_WEDGE_CONSISTENCY", "1"),
        ("RUSTDL_TRACE", "1"),
    ];
    let ce = ":C and (:s some owl:Thing) and (:s only (not :E))";
    let cases: [(&[&str], &str); 6] = [
        (&["sat-expr", "--json", &f, ce], "sat-expr"),
        // Text mode keeps the decided answer too (Fable's review of #500: it used to turn
        // into `unknown`).
        (&["sat-expr", &f, ce], "unsatisfiable"),
        (&["subclass-expr", &f, ce, ":Y"], "yes"),
        (&["sat", &f, "http://ex/#U"], "unsat"),
        (&["subclass", &f, "http://ex/#X", "http://ex/#Y"], "yes"),
        (&["explain", &f, "http://ex/#X", "http://ex/#Y"], "yes"),
    ];
    for (args, answer) in cases {
        let what = args.join(" ");
        let r = run_with(false, &env, args);
        let line = lazy_line(&r);
        assert!(
            line.contains("route=prepared") && line.contains("tail=Some(None)"),
            "{what}: precondition, the prepared route's tail ran and gave up: {line}"
        );
        assert!(line.contains("tail_stalled=true"), "{what}: {line}");
        assert_gave_up(&r, &what);
        if args.contains(&"--json") {
            let j = json(&r, &what);
            assert_eq!(j["satisfiable"], false, "{what}: the answer stands: {j}");
            assert_eq!(j["consistency_undetermined"], true, "{what}: {j}");
            assert_eq!(j["no_verdict"], true, "{what}: {j}");
            assert_eq!(
                j["incomplete"], false,
                "{what}: the answer was decided: {j}"
            );
        } else {
            assert!(
                r.stdout.contains(answer),
                "{what}: the answer stands: {}",
                r.stdout
            );
            assert!(
                !r.stdout.contains("unknown"),
                "{what}: not `unknown`: {}",
                r.stdout
            );
        }
        // Control: no forced stall, the tail decides "consistent".
        let r = run_with(false, &[("RUSTDL_WEDGE_CONSISTENCY", "1")], args);
        assert_eq!(r.code, 0, "{what} control: {}\n{}", r.stdout, r.stderr);
        assert!(!r.stderr.contains(MARKER), "{what} control: {}", r.stderr);
    }
}

/// #493 (b): the tail with individuals (opt-in, `RUSTDL_QUERY_LAZY_CONSISTENCY_TAIL=1`),
/// the consistency wedge off so the verdict is undetermined. `a : C` is told, so
/// `instance C a` is a closure hit (no probe) and `instances C` lists `a`.
#[test]
fn a_stall_in_the_opt_in_lazy_tail_is_reported() {
    let f = Fixture::with(
        "lazy-tail-abox",
        "Prefix(:=<http://ex/#>)\nPrefix(owl:=<http://www.w3.org/2002/07/owl#>)\n\
         Ontology(<http://ex/aboxtold>\n\
         Declaration(Class(:C)) Declaration(Class(:D)) Declaration(Class(:E))\n\
         Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))\n\
         Declaration(ObjectProperty(:r))\n\
         SubClassOf(owl:Thing :E) SubClassOf(:C ObjectAllValuesFrom(:r :E))\n\
         ClassAssertion(:C :a) ClassAssertion(:D :b) ObjectPropertyAssertion(:r :a :b)\n)\n",
    );
    let env = [
        ("RUSTDL_QUERY_LAZY_CONSISTENCY_TAIL", "1"),
        ("RUSTDL_TRACE", "1"),
    ];
    for (args, answer) in [
        (
            vec!["instance", f.path(), "http://ex/#C", "http://ex/#a"],
            "yes",
        ),
        (vec!["instances", f.path(), "http://ex/#C"], "http://ex/#a"),
    ] {
        let what = args.join(" ");
        let r = run_with(true, &env, &args);
        let line = lazy_line(&r);
        assert!(
            line.contains("route=prepared") && line.contains("tail=Some(None)"),
            "{what}: precondition: {line}"
        );
        assert!(line.contains("tail_stalled=true"), "{what}: {line}");
        assert_gave_up(&r, &what);
        assert!(
            r.stdout.contains(answer),
            "{what}: the answer stands: {}",
            r.stdout
        );
        let r = run_with(false, &env, &args);
        assert_eq!(r.code, 0, "{what} control: {}\n{}", r.stdout, r.stderr);
        assert!(!r.stderr.contains(MARKER), "{what} control: {}", r.stderr);
    }
}

/// #493 (a): the #344 data-counting verify. The consistency wedge answers `Sat` but skips
/// data counting, so its model is verified by a main-tableau probe, which stalls. `a : C` is
/// told (a closure hit). The opt-in tail is on, to show it is skipped after a verify stall
/// (the same probe would stall again).
#[test]
fn a_stall_in_the_lazy_data_counting_verify_is_reported() {
    let f = Fixture::with(
        "lazy-verify",
        "Prefix(:=<http://ex/#>)\nPrefix(owl:=<http://www.w3.org/2002/07/owl#>)\n\
         Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)\n\
         Ontology(<http://ex/datacount>\n\
         Declaration(Class(:C)) Declaration(Class(:P)) Declaration(Class(:E))\n\
         Declaration(NamedIndividual(:a)) Declaration(DataProperty(:p))\n\
         SubClassOf(owl:Thing :E) SubClassOf(:P DataMaxCardinality(2 :p xsd:integer))\n\
         ClassAssertion(:P :a) ClassAssertion(:C :a)\n\
         DataPropertyAssertion(:p :a \"1\"^^xsd:integer)\n)\n",
    );
    let env = [
        ("RUSTDL_WEDGE_CONSISTENCY", "1"),
        ("RUSTDL_QUERY_LAZY_CONSISTENCY_TAIL", "1"),
        ("RUSTDL_TRACE", "1"),
    ];
    let args = ["instance", f.path(), "http://ex/#C", "http://ex/#a"];
    let r = run_with(true, &env, &args);
    let line = lazy_line(&r);
    assert!(
        line.contains("route=prepared") && line.contains("settle_stalled=true"),
        "precondition: the verify stalled: {line}"
    );
    assert!(
        line.contains("tail=None"),
        "the tail is skipped after a verify stall: {line}"
    );
    assert_gave_up(&r, "instance, verify stall");
    assert_eq!(r.stdout.trim(), "yes", "the answer stands: {}", r.stdout);
    let r = run_with(false, &env, &args);
    assert_eq!(r.code, 0, "control: {}\n{}", r.stdout, r.stderr);
    assert!(!r.stderr.contains(MARKER), "control: {}", r.stderr);
}

/// #493: the lazy tail's routine decision-depth cut is NOT the signal. The exhausting
/// 280-union under `⊤` (every `Bk` disjoint from `X`), no individual: the tail's
/// consistency probe goes down the `Ak` choices to the depth limit and every alternative
/// clashes, well before its deadline (early abandon off, so this is `map_depth_limit`'s
/// pure depth-cap arm). Undetermined, no field, no note, exit 0. The trace's
/// `tail_depth_capped=true` and the elapsed time are the positive instrument.
#[test]
fn a_depth_cut_in_the_lazy_tail_is_not_a_no_verdict() {
    let body: Vec<String> = (0..280)
        .map(|k| format!("ObjectUnionOf(:A{k} :B{k})"))
        .collect();
    let disjoint: Vec<String> = (0..280)
        .map(|k| format!("DisjointClasses(:B{k} :X)"))
        .collect();
    let src = format!(
        "Prefix(:=<http://example.org/d#>)\nPrefix(owl:=<http://www.w3.org/2002/07/owl#>)\n\
         Ontology(<http://example.org/d>\nDeclaration(Class(:C)) Declaration(Class(:Y))\n\
         Declaration(ObjectProperty(:s))\nSubClassOf(owl:Thing :X)\n\
         SubClassOf(:X ObjectIntersectionOf({}))\n{}\n)\n",
        body.join(" "),
        disjoint.join("\n")
    );
    let f = Fixture::with("lazy-depthcut", &src);
    let env = [
        ("RUSTDL_TABLEAU_EARLY_ABANDON", "0"),
        ("RUSTDL_WEDGE_CONSISTENCY", "1"),
        ("RUSTDL_TRACE", "1"),
    ];
    let t = std::time::Instant::now();
    let r = run_with(
        false,
        &env,
        &[
            "sat-expr",
            "--json",
            f.path(),
            ":C and (:s some owl:Thing) and (:s only (not :X))",
        ],
    );
    let line = lazy_line(&r);
    assert!(
        line.contains("tail=Some(None)") && line.contains("tail_depth_capped=true"),
        "precondition: the tail ended on the depth cap: {line}"
    );
    assert!(line.contains("tail_stalled=false"), "{line}");
    assert!(
        t.elapsed() < std::time::Duration::from_secs(8),
        "the tail must end on the depth limit, well before its deadline: {:?}",
        t.elapsed()
    );
    assert_eq!(r.code, 0, "{}\n{}", r.stdout, r.stderr);
    assert!(
        !r.stderr.contains(MARKER),
        "a depth cut is not the bug signal: {}",
        r.stderr
    );
    let j = json(&r, "sat-expr");
    assert_eq!(j["satisfiable"], false, "{j}");
    assert_eq!(j["consistency_undetermined"], true, "{j}");
    assert_eq!(j["no_verdict"], false, "{j}");
    // Text mode prints the consistency-undetermined warning; it must not carry #449's
    // marker phrase, or every harness that detects the note by it would read a plain
    // "undetermined" as the bug signal (found by the #493 census).
    let r = run_with(
        false,
        &env,
        &[
            "sat-expr",
            f.path(),
            ":C and (:s some owl:Thing) and (:s only (not :X))",
        ],
    );
    assert!(
        r.stderr.contains("consistency could not be determined"),
        "the warning printed: {}",
        r.stderr
    );
    assert_eq!(r.code, 0, "{}\n{}", r.stdout, r.stderr);
    assert!(
        !r.stderr.contains(MARKER),
        "the warning is not the note: {}",
        r.stderr
    );
}

/// #493, a documented gap (`docs/known-limitations/tbox-only-inconsistency-light-route.md`):
/// the property-disjointness answers keep only `consistency_undetermined` from the light
/// route's opt-in tail, with no `no_verdict` and no note. One class only (`⊤ ⊑ E`), so
/// `disjoint` probes no class pair; the told `DisjointObjectProperties` is the positive
/// answer. When this is fixed, flip it.
#[test]
fn a_stall_in_the_light_lazy_tail_is_not_reported() {
    let f = Fixture::with(
        "lazy-light-gap",
        "Prefix(:=<http://ex/#>)\nPrefix(owl:=<http://www.w3.org/2002/07/owl#>)\n\
         Ontology(<http://ex/lightgap>\n\
         Declaration(Class(:E)) Declaration(ObjectProperty(:r)) Declaration(ObjectProperty(:s))\n\
         Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))\n\
         SubClassOf(owl:Thing :E) DisjointObjectProperties(:r :s)\n\
         ObjectPropertyAssertion(:r :a :b)\n)\n",
    );
    let env = [
        ("RUSTDL_QUERY_LAZY_CONSISTENCY_TAIL", "1"),
        ("RUSTDL_TRACE", "1"),
    ];
    let r = run_with(true, &env, &["disjoint", "--json", f.path()]);
    let line = lazy_line(&r);
    assert!(
        line.contains("route=light") && line.contains("tail_stalled=true"),
        "precondition: the light route's tail stalled: {line}"
    );
    assert_eq!(r.code, 0, "{}\n{}", r.stdout, r.stderr);
    assert!(!r.stderr.contains(MARKER), "the gap: no note: {}", r.stderr);
    let j = json(&r, "disjoint");
    assert_eq!(j["consistency_undetermined"], true, "{j}");
    assert_eq!(j["no_verdict"], false, "the gap: no field: {j}");
}

/// #493: the light route (an answer without the tableau, here the closure's `C ⊓ ¬C`) with
/// its opt-in tail: `sat-expr` reports the tail's stall like the prepared route's.
#[test]
fn a_stall_in_the_light_lazy_tail_is_reported_by_sat_expr() {
    let f = Fixture::with(
        "lazy-light-satexpr",
        "Prefix(:=<http://ex/#>)\nPrefix(owl:=<http://www.w3.org/2002/07/owl#>)\n\
         Ontology(<http://ex/lightsat>\n\
         Declaration(Class(:C)) Declaration(Class(:E)) Declaration(ObjectProperty(:r))\n\
         Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))\n\
         SubClassOf(owl:Thing :E) SubClassOf(:C ObjectAllValuesFrom(:r :E))\n\
         ClassAssertion(:C :a) ObjectPropertyAssertion(:r :a :b)\n)\n",
    );
    let env = [
        ("RUSTDL_QUERY_LAZY_CONSISTENCY_TAIL", "1"),
        ("RUSTDL_TRACE", "1"),
    ];
    let args = ["sat-expr", "--json", f.path(), ":C and not :C"];
    let r = run_with(true, &env, &args);
    let line = lazy_line(&r);
    assert!(
        line.contains("route=light") && line.contains("tail_stalled=true"),
        "precondition: the light route's tail stalled: {line}"
    );
    assert_gave_up(&r, "sat-expr, light tail");
    let j = json(&r, "sat-expr");
    assert_eq!(j["satisfiable"], false, "the answer stands: {j}");
    assert_eq!(j["consistency_undetermined"], true, "{j}");
    assert_eq!(j["no_verdict"], true, "{j}");
    // Text mode: the decided answer, then the note, exit 3 (Fable's review of #500).
    let text = ["sat-expr", f.path(), ":C and not :C"];
    let r = run_with(true, &env, &text);
    assert_gave_up(&r, "sat-expr (text), light tail");
    assert_eq!(
        r.stdout.trim(),
        "unsatisfiable",
        "the answer stands: {}",
        r.stdout
    );
    let r = run_with(false, &env, &args);
    assert_eq!(r.code, 0, "control: {}\n{}", r.stdout, r.stderr);
}
