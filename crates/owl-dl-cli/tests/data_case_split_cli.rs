//! #589: a data range that may need reasoning by cases is reported on every surface.
//!
//! Data ranges lower to atomic keys and no engine splits a key into its values, so
//! `C ⊑ ∃d.{1,2}`, `hasValue(d,1) ⊑ E`, `hasValue(d,2) ⊑ E` miss `C ⊑ E` (`HermiT`
//! derives it) while the pure-EL gate admits all three. The answers are unchanged; the
//! JSON outputs carry `data_case_split_risk: true`, `completeness_guaranteed` is false
//! (condition 12), the text outputs print one stderr note, `explain` drops its
//! completeness claim, and `verify-el` turns `Verified` into `Unresolved` (exit 3). The
//! negative control (points only) keeps everything clean.
#![allow(clippy::unwrap_used)]

use std::path::PathBuf;
use std::process::Command;

const T: &str = "http://ex.org/t#";
const NOTE: &str = "may need reasoning by cases";

static NEXT_FIXTURE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

struct Fixture(PathBuf);

impl Fixture {
    fn new(tag: &str, body: &str) -> Self {
        let n = NEXT_FIXTURE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "rustdl-data-case-split-{}-{n}-{tag}.ofn",
            std::process::id()
        ));
        let src = format!(
            "Prefix(:=<{T}>)\nPrefix(owl:=<http://www.w3.org/2002/07/owl#>)\n\
             Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)\n\
             Ontology(<http://ex.org/t>\n\
             Declaration(Class(:C)) Declaration(Class(:E)) Declaration(DataProperty(:d))\n\
             Declaration(NamedIndividual(:a))\n{body}\n)\n"
        );
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

/// (exit code, stdout, stderr).
fn run(args: &[&str]) -> (i32, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_rustdl"))
        .args(args)
        .env_remove("RUSTDL_TRACE")
        .output()
        .unwrap();
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8(out.stdout).unwrap(),
        String::from_utf8(out.stderr).unwrap(),
    )
}

fn json(stdout: &str) -> serde_json::Value {
    serde_json::from_str(stdout).unwrap()
}

/// The issue's repro.
const SPLIT: &str = "SubClassOf(:C DataSomeValuesFrom(:d DataOneOf(\"1\"^^xsd:integer \"2\"^^xsd:integer)))\n\
     SubClassOf(DataHasValue(:d \"1\"^^xsd:integer) :E)\n\
     SubClassOf(DataHasValue(:d \"2\"^^xsd:integer) :E)";
/// Points only: nothing to split.
const POINTS: &str = "SubClassOf(:C DataHasValue(:d \"1\"^^xsd:integer))\n\
     SubClassOf(DataHasValue(:d \"1\"^^xsd:integer) :E)";

#[test]
fn points_only_raise_nothing() {
    let f = Fixture::new("points", POINTS);
    let (code, out, err) = run(&["classify", "--json", f.path()]);
    assert_eq!(code, 0, "{err}");
    let j = json(&out);
    assert_eq!(j["data_case_split_risk"], false);
    assert_eq!(j["completeness_guaranteed"], true);
    assert!(!err.contains(NOTE), "{err}");
    let (code, out, _) = run(&["verify-el", "--json", f.path()]);
    assert_eq!(code, 0, "verify-el: {out}");
    let (_, out, _) = run(&["unlowered", f.path()]);
    assert_eq!(json(&out)["data_case_split_risk"], false);
}

#[test]
fn classify_reports_the_case_split_in_its_own_field() {
    let f = Fixture::new("classify", SPLIT);
    let (code, out, err) = run(&["classify", "--json", f.path()]);
    assert_eq!(code, 0, "{err}");
    let j = json(&out);
    assert_eq!(j["data_case_split_risk"], true);
    assert_eq!(j["completeness_guaranteed"], false);
    assert_eq!(j["incomplete"], false, "nothing was cut");
    assert_eq!(err.matches(NOTE).count(), 1, "one note: {err}");
    // Plain output too.
    let (_, _, err) = run(&["classify", f.path()]);
    assert_eq!(err.matches(NOTE).count(), 1, "plain classify: {err}");
}

#[test]
fn the_other_surfaces_carry_the_flag() {
    let f = Fixture::new("surfaces", &format!("{SPLIT}\nClassAssertion(:C :a)"));
    for args in [vec!["consistent", "--json"], vec!["realize", "--json"]] {
        let mut a = args.clone();
        a.push(f.path());
        let (_, out, err) = run(&a);
        assert_eq!(json(&out)["data_case_split_risk"], true, "{args:?}: {out}");
        assert!(err.contains(NOTE), "{args:?}: {err}");
    }
    for (cmd, expr) in [
        ("sat-expr", vec![":C"]),
        ("instances-expr", vec![":E"]),
        ("subclass-expr", vec![":C", ":E"]),
    ] {
        let mut a = vec![cmd, "--json", f.path()];
        a.extend(expr.iter().copied());
        let (_, out, _) = run(&a);
        let j = json(&out);
        assert_eq!(j["data_case_split_risk"], true, "{cmd}: {out}");
        if j.get("completeness_guaranteed").is_some() {
            assert_eq!(j["completeness_guaranteed"], false, "{cmd}: {out}");
        }
    }
    let (_, out, _) = run(&["unlowered", f.path()]);
    assert_eq!(json(&out)["data_case_split_risk"], true);
}

#[test]
fn explain_drops_its_completeness_claim() {
    let f = Fixture::new("explain", SPLIT);
    let (_, out, err) = run(&["explain", f.path(), &format!("{T}C"), &format!("{T}E")]);
    assert!(
        !out.contains("closure is complete)"),
        "explain must not claim completeness: {out}"
    );
    assert!(out.contains("NOT complete"), "{out}");
    assert!(err.contains(NOTE), "{err}");
}

#[test]
fn verify_el_is_unresolved() {
    let f = Fixture::new("verify", SPLIT);
    let (code, out, _) = run(&["verify-el", "--json", f.path()]);
    assert_eq!(code, 3, "Verified must turn Unresolved: {out}");
    assert!(out.contains("DataKeyNeedsCaseSplit"), "{out}");
}
