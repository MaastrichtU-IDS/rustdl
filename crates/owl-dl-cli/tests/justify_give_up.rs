//! #396: `justify`, `repair` and `diagnose` must not read a reasoner give-up
//! as "not entailed".
//!
//! Each of their entailment probes used to fold a live-node-cap trip
//! (`RUSTDL_MAX_NODES`) into "not entailed" with nothing telling the caller. A
//! TOP-LEVEL give-up now prints `unknown` and exits 3 (the #363 A2 convention);
//! a give-up inside the search keeps the (sound) result, sets `incomplete`, and
//! warns on stderr.
//!
//! Each case runs the binary in its own process, so the `OnceLock`-cached
//! `RUSTDL_MAX_NODES` is read fresh every time. The control arm sets the cap to
//! `0` (disabled) and must give today's answer.
#![allow(clippy::unwrap_used)]

use std::path::PathBuf;
use std::process::Command;

// The #363 A2 fixture: C and D are satisfiable and unrelated, but each generates
// an `:r`-successor, so a tableau probe needs a node beyond the root and trips
// `RUSTDL_MAX_NODES=1`. `∀s.E` takes the ontology out of EL so the tableau runs
// on the full set (the query's ⊥-module drops it, and the module's EL "no" is
// re-checked on the full set).
const TOP_LEVEL: &str = r"Prefix(:=<http://ex/#>)
Ontology(<http://ex/justifygiveup>
  Declaration(Class(:C)) Declaration(Class(:D)) Declaration(Class(:E))
  Declaration(ObjectProperty(:r)) Declaration(ObjectProperty(:s))
  Declaration(NamedIndividual(:a))
  SubClassOf(:C ObjectSomeValuesFrom(:r :C))
  SubClassOf(:D ObjectSomeValuesFrom(:r :D))
  SubClassOf(:C ObjectAllValuesFrom(:s :E))
  ClassAssertion(:C :a)
)
";

// C ⊑ D has two justifications, {C ⊑ D} and {C ⊑ X, X ⊑ D}, both in the EL
// closure, so the top-level probe answers without the tableau. `C ⊑ ∃r.C` and
// `C ⊑ ∀r.E` are in the query's ⊥-module (r enters through C), so the
// enumeration's branch probes without a justification go to the tableau and trip
// the cap: the search gives up below the top level, and a justification may be
// missing. (`QuickXplain` itself never probes such a subset here, because the
// canonical order puts the two justifications first.)
const BRANCH: &str = r"Prefix(:=<http://ex/#>)
Ontology(<http://ex/justifysubset>
  Declaration(Class(:C)) Declaration(Class(:X)) Declaration(Class(:D)) Declaration(Class(:E))
  Declaration(ObjectProperty(:r))
  SubClassOf(:C :X)
  SubClassOf(:X :D)
  SubClassOf(:C :D)
  SubClassOf(:C ObjectSomeValuesFrom(:r :C))
  SubClassOf(:C ObjectAllValuesFrom(:r :E))
)
";

// C is unsatisfiable (its r-successor is A or B, and both are excluded), D is
// not. With the hyper wedge off, classify's unsat probe on C needs the tableau
// and trips the cap, so C is reported satisfiable without a verdict.
const UNSAT_CUT: &str = r"Prefix(:=<http://ex/dc#>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Ontology(<http://ex/diagcut>
  Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C)) Declaration(Class(:D))
  Declaration(ObjectProperty(:r))
  SubClassOf(:C ObjectSomeValuesFrom(:r ObjectUnionOf(:A :B)))
  SubClassOf(ObjectSomeValuesFrom(:r :A) owl:Nothing)
  SubClassOf(ObjectSomeValuesFrom(:r :B) owl:Nothing)
)
";

// C ⊑ D has two justifications: {C ⊑ D}, in the EL closure, and
// {C ⊑ ∃r.(A ⊔ B), ∃r.A ⊑ D, ∃r.B ⊑ D}, which needs the tableau (with the hyper
// wedge off). At the cap the enumeration finds only {C ⊑ D}, so the one
// candidate repair is "remove C ⊑ D" — which does NOT break the entailment
// (HermiT agrees). Its verification probe gives up, and used to accept it.
const REPAIR: &str = r"Prefix(:=<http://ex/rg#>)
Ontology(<http://ex/repairgiveup>
  Declaration(Class(:C)) Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:D))
  Declaration(ObjectProperty(:r))
  SubClassOf(:C :D)
  SubClassOf(:C ObjectSomeValuesFrom(:r ObjectUnionOf(:A :B)))
  SubClassOf(ObjectSomeValuesFrom(:r :A) :D)
  SubClassOf(ObjectSomeValuesFrom(:r :B) :D)
)
";

// ⊤ ⊑ ∃r.(A ⊔ B) with both ∃r.A and ∃r.B unsatisfiable: ⊤ is unsatisfiable, so
// every class is, and the TBox-only ontology is inconsistent. `classify` does
// not flag that, so `diagnose` asks `is_consistent`, whose tableau trips the cap.
// The same shape with the justification written as `EquivalentClasses`, which
// sorts AFTER `SubClassOf`: `QuickXplain`'s first split then probes
// {C ⊑ ∃r.C, C ⊑ ∀r.E} alone, which needs the tableau and trips the cap. The
// result still entails, but its minimality is no longer guaranteed.
const QX: &str = r"Prefix(:=<http://ex/#>)
Ontology(<http://ex/justifyqx>
  Declaration(Class(:C)) Declaration(Class(:X)) Declaration(Class(:D)) Declaration(Class(:E))
  Declaration(ObjectProperty(:r))
  SubClassOf(:C ObjectSomeValuesFrom(:r :C))
  SubClassOf(:C ObjectAllValuesFrom(:r :E))
  EquivalentClasses(:C :X)
  EquivalentClasses(:X :D)
)
";

const ALL_UNSAT: &str = r"Prefix(:=<http://ex/au#>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Ontology(<http://ex/allunsat>
  Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C))
  Declaration(ObjectProperty(:r))
  SubClassOf(owl:Thing ObjectSomeValuesFrom(:r ObjectUnionOf(:A :B)))
  SubClassOf(ObjectSomeValuesFrom(:r :A) owl:Nothing)
  SubClassOf(ObjectSomeValuesFrom(:r :B) owl:Nothing)
)
";

// `SubDataPropertyOf(sub, sup)` is NOT entailed (HermiT agrees): only the value 0
// is forced from `sub` to `sup`, and `sub`'s value 0 is impossible (it forces G,
// whose r-chain ends in P ⊔ Q, both empty). The probe's guard asserts
// `sub(_a, 0)` alone — inconsistent through that chain — and so answers "not
// entailed". At cap 7 (wedge off) the guard's tableau trips the cap while c2
// (`sub(_a, 0)`, `¬sup(_a, 0)`) still finds its clash; main 293dcfd1 read the
// guard's give-up as "consistent" and printed a justification.
const DATA_GUARD: &str = r#"Prefix(:=<http://ex/dg#>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)
Ontology(<http://ex/dataguard>
  Declaration(DataProperty(:sub)) Declaration(DataProperty(:sup))
  Declaration(ObjectProperty(:r))
  Declaration(Class(:G)) Declaration(Class(:H1)) Declaration(Class(:H2))
  Declaration(Class(:H3)) Declaration(Class(:H4)) Declaration(Class(:P)) Declaration(Class(:Q))
  SubClassOf(DataHasValue(:sub "0"^^xsd:integer) DataHasValue(:sup "0"^^xsd:integer))
  SubClassOf(DataHasValue(:sub "0"^^xsd:integer) :G)
  SubClassOf(:G ObjectSomeValuesFrom(:r :H1))
  SubClassOf(:H1 ObjectSomeValuesFrom(:r :H2))
  SubClassOf(:H2 ObjectSomeValuesFrom(:r :H3))
  SubClassOf(:H3 ObjectSomeValuesFrom(:r :H4))
  SubClassOf(:H4 ObjectUnionOf(:P :Q))
  SubClassOf(:P owl:Nothing)
  SubClassOf(:Q owl:Nothing)
)
"#;

/// Writes a fixture to a per-test file (tests run in parallel).
struct Fixture(PathBuf);

impl Fixture {
    fn new(src: &str, tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "rustdl-justify-give-up-{}-{tag}.ofn",
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

fn run(cap: &str, args: &[&str]) -> Run {
    run_with(cap, &[], args)
}

fn run_with(cap: &str, extra_env: &[(&str, &str)], args: &[&str]) -> Run {
    let out = Command::new(env!("CARGO_BIN_EXE_rustdl"))
        .args(args)
        .envs(extra_env.iter().copied())
        .env("RUSTDL_MAX_NODES", cap)
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

#[test]
fn justify_prints_unknown_when_the_top_level_probe_gives_up() {
    let f = Fixture::new(TOP_LEVEL, "top-text");
    let capped = run("1", &["justify", f.path(), "subclass", C, D]);
    assert!(capped.stdout.starts_with("unknown"), "{}", capped.stdout);
    assert_eq!(capped.code, 3, "stderr: {}", capped.stderr);
    assert!(
        capped.stderr.contains("not a non-entailment"),
        "{}",
        capped.stderr
    );
    let control = run("0", &["justify", f.path(), "subclass", C, D]);
    assert_eq!(control.stdout.trim(), "not entailed (no justification)");
    assert_eq!(control.code, 0, "stderr: {}", control.stderr);
}

#[test]
fn justify_json_reports_unknown_when_the_top_level_probe_gives_up() {
    let f = Fixture::new(TOP_LEVEL, "top-json");
    for all in [false, true] {
        let mut args = vec!["justify", f.path(), "--json"];
        if all {
            args.push("--all");
        }
        args.extend(["subclass", C, D]);
        let capped = run("1", &args);
        let j = capped.json();
        assert_eq!(j["status"], "unknown", "all={all}: {j}");
        assert_eq!(j["incomplete"], true, "all={all}: {j}");
        assert_eq!(capped.code, 3, "all={all}");
        let control = run("0", &args);
        let j = control.json();
        assert_eq!(j["status"], "not-entailed", "all={all}: {j}");
        assert_eq!(j["incomplete"], false, "all={all}: {j}");
        assert_eq!(control.code, 0, "all={all}");
    }
}

#[test]
fn justify_all_json_flags_a_branch_probe_give_up() {
    let f = Fixture::new(BRANCH, "branch-json");
    let args = ["justify", f.path(), "--json", "--all", "subclass", C, D];
    let capped = run("1", &args);
    let j = capped.json();
    // The result stays sound: entailed, and every justification shown entails.
    assert_eq!(j["status"], "entailed", "{j}");
    assert!(!j["justifications"].as_array().unwrap().is_empty(), "{j}");
    assert_eq!(j["incomplete"], true, "{j}");
    // A pruned branch can hide a justification, not unminimize a found one:
    // each `QuickXplain` here finished without a give-up.
    assert_eq!(j["enumeration_complete"], false, "{j}");
    assert_eq!(j["minimal"], true, "{j}");
    assert_eq!(capped.code, 0);
    assert!(
        capped.stderr.contains("gave up during the search"),
        "{}",
        capped.stderr
    );

    let control = run("0", &args);
    let j = control.json();
    assert_eq!(j["status"], "entailed", "{j}");
    assert_eq!(j["justifications"].as_array().unwrap().len(), 2, "{j}");
    assert_eq!(j["incomplete"], false, "{j}");
    assert_eq!(j["minimal"], true, "{j}");
    assert_eq!(j["enumeration_complete"], true, "{j}");
    assert!(control.stderr.is_empty(), "{}", control.stderr);
}

#[test]
fn justify_flags_a_quickxplain_give_up_as_not_minimal() {
    let f = Fixture::new(QX, "qx");
    let args = ["justify", f.path(), "--json", "subclass", C, D];
    let capped = run("1", &args);
    let j = capped.json();
    assert_eq!(j["status"], "entailed", "{j}");
    assert_eq!(j["incomplete"], true, "{j}");
    assert_eq!(j["minimal"], false, "{j}");
    assert_eq!(capped.code, 0);
    assert!(
        capped.stderr.contains("gave up during the search"),
        "{}",
        capped.stderr
    );
    let text = run("1", &["justify", f.path(), "subclass", C, D]);
    assert!(
        text.stdout.contains("minimality NOT guaranteed"),
        "{}",
        text.stdout
    );
    let control = run("0", &args);
    let j = control.json();
    assert_eq!(j["incomplete"], false, "{j}");
    assert_eq!(j["minimal"], true, "{j}");
}

#[test]
fn justify_laconic_survives_a_give_up_inside_the_search() {
    // The test binary is a debug build, so laconic's entailment `debug_assert!`
    // runs: a give-up there must not read as a definite "no" and panic.
    let f = Fixture::new(BRANCH, "laconic");
    let args = [
        "justify",
        f.path(),
        "--json",
        "--all",
        "--laconic",
        "subclass",
        C,
        D,
    ];
    let capped = run("1", &args);
    assert_eq!(capped.code, 0, "stderr: {}", capped.stderr);
    let j = capped.json();
    assert_eq!(j["status"], "entailed", "{j}");
    assert_eq!(j["incomplete"], true, "{j}");
    let control = run("0", &args);
    assert_eq!(control.json()["incomplete"], false);
}

#[test]
fn justify_property_kind_prints_unknown_when_its_consistency_probe_gives_up() {
    // `subproperty` reduces to an inconsistency probe. The consistency wedge
    // would decide it without the tableau, so turn it off.
    let f = Fixture::new(TOP_LEVEL, "property");
    let wedge_off = [("RUSTDL_WEDGE_CONSISTENCY", "0")];
    let args = [
        "justify",
        f.path(),
        "subproperty",
        "http://ex/#r",
        "http://ex/#s",
    ];
    let capped = run_with("1", &wedge_off, &args);
    assert!(capped.stdout.starts_with("unknown"), "{}", capped.stdout);
    assert_eq!(capped.code, 3);
    let control = run_with("0", &wedge_off, &args);
    assert_eq!(control.stdout.trim(), "not entailed (no justification)");
    assert_eq!(control.code, 0);
}

#[test]
fn repair_prints_unknown_when_the_top_level_probe_gives_up() {
    // Today this printed "not entailed; nothing to repair".
    let f = Fixture::new(TOP_LEVEL, "repair-top");
    let capped = run("1", &["repair", f.path(), "subclass", C, D]);
    assert!(capped.stdout.starts_with("unknown"), "{}", capped.stdout);
    assert_eq!(capped.code, 3);
    let control = run("0", &["repair", f.path(), "subclass", C, D]);
    assert_eq!(control.stdout.trim(), "not entailed; nothing to repair");
    assert_eq!(control.code, 0);
}

#[test]
fn repair_does_not_claim_completeness_after_a_give_up() {
    let f = Fixture::new(BRANCH, "repair-subset");
    let capped = run("1", &["repair", f.path(), "subclass", C, D]);
    assert_eq!(capped.code, 0, "stderr: {}", capped.stderr);
    assert!(
        capped.stderr.contains("gave up during the search"),
        "{}",
        capped.stderr
    );
    // The verification probes give up here too, so no repair is verified; none
    // may be presented as complete either way.
    assert!(!capped.stdout.contains("— complete"), "{}", capped.stdout);
    assert!(
        capped.stdout.contains("verification gave up"),
        "{}",
        capped.stdout
    );
    let control = run("0", &["repair", f.path(), "subclass", C, D]);
    assert!(control.stdout.contains("— complete"), "{}", control.stdout);
    assert!(control.stderr.is_empty(), "{}", control.stderr);
}

#[test]
fn diagnose_does_not_present_a_given_up_consistency_check_as_a_verdict() {
    let f = Fixture::new(ALL_UNSAT, "diagnose");
    let capped = run("1", &["diagnose", f.path()]);
    assert_eq!(capped.code, 0, "stderr: {}", capped.stderr);
    assert!(
        capped.stdout.contains("consistent (NOT witnessed"),
        "{}",
        capped.stdout
    );
    assert!(
        capped.stderr.contains("consistency check gave up"),
        "{}",
        capped.stderr
    );
    let control = run("0", &["diagnose", f.path()]);
    assert!(
        control.stdout.contains("# consistency: INCONSISTENT"),
        "{}",
        control.stdout
    );
}

#[test]
fn repair_never_reports_a_removal_whose_verification_gave_up() {
    // The hyper wedge would prove the hidden justification without the tableau.
    let f = Fixture::new(REPAIR, "repair-verify");
    let wedge_off = [("RUSTDL_HYPERTABLEAU", "0")];
    let args = [
        "repair",
        f.path(),
        "subclass",
        "http://ex/rg#C",
        "http://ex/rg#D",
    ];
    let capped = run_with("1", &wedge_off, &args);
    assert_eq!(capped.code, 0, "stderr: {}", capped.stderr);
    // On 293dcfd1 this printed "repair 1 (remove 1 axiom(s)): C SubClassOf D".
    assert!(!capped.stdout.contains("repair 1"), "{}", capped.stdout);
    assert!(
        capped.stdout.contains("no verifiable axiom removal found"),
        "{}",
        capped.stdout
    );
    assert!(
        capped
            .stdout
            .contains("1 candidate(s) dropped (verification gave up"),
        "{}",
        capped.stdout
    );
    assert!(
        capped.stderr.contains("gave up during the search"),
        "{}",
        capped.stderr
    );

    // Uncapped, all three repairs are found and verified.
    let control = run_with("0", &wedge_off, &args);
    assert!(
        control.stdout.contains("# 3 minimal repair(s)"),
        "{}",
        control.stdout
    );
    assert!(
        !control.stdout.contains("verification gave up"),
        "{}",
        control.stdout
    );
}

#[test]
fn diagnose_does_not_report_coherence_after_a_cut_unsat_probe() {
    // The hyper wedge would prove C unsatisfiable without the tableau.
    let f = Fixture::new(UNSAT_CUT, "diagnose-cut");
    let wedge_off = [("RUSTDL_HYPERTABLEAU", "0")];
    let capped = run_with("1", &wedge_off, &["diagnose", f.path()]);
    assert_eq!(capped.code, 0, "stderr: {}", capped.stderr);
    // On 293dcfd1 this printed "# coherent: no unsatisfiable classes".
    assert!(
        !capped.stdout.contains("no unsatisfiable classes"),
        "{}",
        capped.stdout
    );
    assert!(
        capped.stdout.contains("check(s) gave up"),
        "{}",
        capped.stdout
    );
    assert!(
        capped.stderr.contains("hit the node cap"),
        "{}",
        capped.stderr
    );
    let control = run_with("0", &wedge_off, &["diagnose", f.path()]);
    assert!(
        control.stdout.contains("# unsatisfiable: 1"),
        "{}",
        control.stdout
    );
    assert!(control.stderr.is_empty(), "{}", control.stderr);
}

/// The guard fix end to end. Release-only: in a debug build, c2's
/// `NegativeDataPropertyAssertion` reaches the tableau as a non-NNF label and
/// trips `add_label`'s `debug_assert!` (#403, a separate, pre-existing bug, on
/// main too, with or without a cap), so the window cannot be reached there. The
/// decision itself is unit-tested in `justify::verdict_tests`. Run:
///
/// ```sh
/// cargo test --release -p owl-dl-cli --test justify_give_up -- --ignored
/// ```
#[test]
#[ignore = "release-only: in a debug build the probe's NegativeDataPropertyAssertion \
            hits the pre-existing non-NNF debug_assert of #403 before the \
            give-up window; the decision is unit-tested in justify::verdict_tests"]
fn a_data_property_guard_give_up_never_reports_an_entailment() {
    // The consistency wedge would decide both probes without the tableau.
    let f = Fixture::new(DATA_GUARD, "data-guard");
    let wedge_off = [("RUSTDL_WEDGE_CONSISTENCY", "0")];
    let args = [
        "justify",
        f.path(),
        "--json",
        "subdata-property",
        "http://ex/dg#sub",
        "http://ex/dg#sup",
    ];
    let control = run_with("0", &wedge_off, &args);
    assert_eq!(control.json()["status"], "not-entailed");
    // Cap 7 is where main printed a justification.
    let window = run_with("7", &wedge_off, &args);
    assert_eq!(window.json()["status"], "unknown", "{}", window.stdout);
    assert_eq!(window.code, 3);
    // The window is one cap wide; sweep around it so a shift in node accounting
    // cannot move a false entailment past the assertion above unseen.
    for cap in 1..=16 {
        let capped = run_with(&cap.to_string(), &wedge_off, &args);
        assert_ne!(capped.json()["status"], "entailed", "cap {cap}");
    }
}
