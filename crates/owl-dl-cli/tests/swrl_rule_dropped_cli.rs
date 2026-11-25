//! #557: a SWRL rule must be counted in `dropped`, so no completeness claim
//! survives it. Before the fix conversion skipped each `DLSafeRule` as benign
//! (`Ok(None)`): `classify --json` reported `completeness_guaranteed: true` and an
//! empty `dropped`, and `instances-expr` claimed a complete answer while missing
//! the instance the rule entails.
//!
//! Every case has a control: the same file without the rule drops nothing and
//! keeps the guarantee, so the rule is the only thing the negative case varies.
#![allow(clippy::unwrap_used)]

use std::path::PathBuf;
use std::process::Command;

const RULE_KIND: &str = "Rule: unsupported axiom (SWRL)";

/// `A ⊑ C`, plus `RULE`. Pure EL and ABox-free once the rule is gone.
const TBOX: &str = "Prefix(:=<http://e#>)
Ontology(
Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C))
SubClassOf(:A :C)
RULE)";

/// As `TBOX` plus `A(a)`. The rule `A(?x) -> B(?x)` entails `B(a)`, which nothing
/// else does (`A ⊑ C` only).
const ABOX: &str = "Prefix(:=<http://e#>)
Ontology(
Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C))
Declaration(NamedIndividual(:a))
SubClassOf(:A :C)
ClassAssertion(:A :a)
RULE)";

/// An unsupported axiom of another kind, for the `--saturation-only` case.
const HASKEY: &str = "Declaration(ObjectProperty(:r)) HasKey(:A (:r) ())";

const RULE: &str = "DLSafeRule(Body(ClassAtom(:A Variable(<urn:swrl:var#x>))) \
                    Head(ClassAtom(:B Variable(<urn:swrl:var#x>))))";

struct Fixture(PathBuf);

impl Fixture {
    fn new(body: &str, rule: bool, tag: &str) -> Self {
        Self::with(body, if rule { RULE } else { "" }, tag)
    }

    fn with(body: &str, extra: &str, tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "rustdl-swrl-dropped-{}-{tag}.ofn",
            std::process::id()
        ));
        let text = body.replace("RULE", extra);
        std::fs::write(&path, text).unwrap();
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
    stdout: String,
    stderr: String,
}

fn run(args: &[&str]) -> Run {
    let out = Command::new(env!("CARGO_BIN_EXE_rustdl"))
        .args(args)
        .output()
        .unwrap();
    assert!(out.status.success(), "{args:?}: {out:?}");
    Run {
        stdout: String::from_utf8(out.stdout).unwrap(),
        stderr: String::from_utf8(out.stderr).unwrap(),
    }
}

fn json(args: &[&str]) -> (serde_json::Value, String) {
    let r = run(args);
    let v = serde_json::from_str(&r.stdout).unwrap_or_else(|e| panic!("{e}: {}", r.stdout));
    (v, r.stderr)
}

#[test]
fn classify_with_a_swrl_rule_counts_it_and_is_not_guaranteed() {
    let f = Fixture::new(TBOX, true, "classify-rule");
    let (v, _) = json(&["classify", "--json", f.path()]);
    assert_eq!(v["dropped"][RULE_KIND], 1, "{v}");
    assert_eq!(v["completeness_guaranteed"], false, "{v}");
    // `classify --json` carries the drop in its `dropped` block; the text mode warns.
    let stderr = run(&["classify", f.path()]).stderr;
    assert!(
        stderr.contains("not understood and dropped") && stderr.contains(RULE_KIND),
        "the CLI must warn about the rule: {stderr:?}"
    );

    let c = Fixture::new(TBOX, false, "classify-norule");
    let (v, _) = json(&["classify", "--json", c.path()]);
    assert_eq!(v["dropped"], serde_json::json!({}), "control: {v}");
    assert_eq!(v["completeness_guaranteed"], true, "control: {v}");
    let stderr = run(&["classify", c.path()]).stderr;
    assert!(!stderr.contains("dropped"), "control: {stderr:?}");
}

#[test]
fn instances_expr_with_a_swrl_rule_is_not_guaranteed() {
    let f = Fixture::new(ABOX, true, "inst-rule");
    let (v, stderr) = json(&["instances-expr", "--json", f.path(), ":B"]);
    // The rule entails `B(a)`; rustdl does not reason with rules, so it misses it.
    assert_eq!(v["instances"], serde_json::json!([]), "precondition: {v}");
    assert_eq!(v["completeness_guaranteed"], false, "{v}");
    assert!(
        stderr.contains("not understood and dropped"),
        "the CLI must warn about the rule: {stderr:?}"
    );

    let c = Fixture::new(ABOX, false, "inst-norule");
    let (v, stderr) = json(&["instances-expr", "--json", c.path(), ":B"]);
    // Without the rule `B` has no instance, and the empty answer is complete.
    assert_eq!(v["instances"], serde_json::json!([]), "control: {v}");
    assert_eq!(v["completeness_guaranteed"], true, "control: {v}");
    assert!(!stderr.contains("dropped"), "control: {stderr:?}");
}

/// Fable review of #577: `classify --saturation-only` (and Python's
/// `classify(saturation_only=True)`) never copied `dropped` into its stats, so a
/// pure-EL `TBox` with a rule, or with `HasKey`, still read `dropped: {}` and
/// `completeness_guaranteed: true` there.
#[test]
fn saturation_only_classify_reports_the_drop() {
    for (what, extra, kind) in [
        ("rule", RULE, RULE_KIND),
        ("HasKey", HASKEY, "HasKey: unsupported axiom (HasKey)"),
    ] {
        let f = Fixture::with(TBOX, extra, &format!("satonly-{what}"));
        let (v, _) = json(&["classify", "--saturation-only", "--json", f.path()]);
        assert_eq!(v["dropped"][kind], 1, "{what}: {v}");
        assert_eq!(v["completeness_guaranteed"], false, "{what}: {v}");
    }

    let c = Fixture::with(TBOX, "", "satonly-none");
    let (v, _) = json(&["classify", "--saturation-only", "--json", c.path()]);
    assert_eq!(v["dropped"], serde_json::json!({}), "control: {v}");
    assert_eq!(v["completeness_guaranteed"], true, "control: {v}");
}
