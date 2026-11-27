//! #619: `justify`, `prove`, `repair`, `diagnose` and `report` warn about dropped
//! axioms, as every other reasoning command does. The fixture's SWRL rule
//! `A(?x) → B(?x)` entails `A ⊑ B`, which conversion drops, so `justify` / `prove`
//! answer "not entailed": exactly the answer the warning must qualify. Every case has
//! a control: the same file without the rule drops nothing and warns nothing.
#![allow(clippy::unwrap_used)]

use std::path::PathBuf;
use std::process::Command;

const RULE_KIND: &str = "Rule: unsupported axiom (SWRL) ×1";
const WARNING: &str = "axiom(s) not understood and dropped";

/// `A ⊑ C`, `D ⊑ ⊥` (an unsatisfiable class for diagnose / report), plus `RULE`.
const SRC: &str = "Prefix(:=<http://e#>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Ontology(
Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C)) Declaration(Class(:D))
SubClassOf(:A :C)
SubClassOf(:D owl:Nothing)
RULE)";

const RULE: &str = "DLSafeRule(Body(ClassAtom(:A Variable(<urn:swrl:var#x>))) \
                    Head(ClassAtom(:B Variable(<urn:swrl:var#x>))))";

const A: &str = "http://e#A";
const B: &str = "http://e#B";
const C: &str = "http://e#C";

struct Fixture(PathBuf);

impl Fixture {
    fn new(src: &str, tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "rustdl-explain-dropped-{}-{tag}.ofn",
            std::process::id()
        ));
        std::fs::write(&path, src).unwrap();
        Self(path)
    }

    /// `SRC` with or without the rule.
    fn rule(rule: bool, tag: &str) -> Self {
        Self::new(&SRC.replace("RULE", if rule { RULE } else { "" }), tag)
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

struct Out {
    code: i32,
    stdout: String,
    stderr: String,
}

fn run(args: &[&str]) -> Out {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_rustdl"));
    for (var, _) in std::env::vars_os() {
        if var.to_string_lossy().starts_with("RUSTDL_") {
            cmd.env_remove(var);
        }
    }
    let out = cmd.args(args).output().unwrap();
    Out {
        code: out.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    }
}

/// Runs `args` (with `PATH` replaced by each fixture) on the rule file and on the
/// control. The rule file warns with `consequence` and names the kind; the control
/// warns nothing.
fn warns_on_the_drop_only(args: &[&str], consequence: &str) -> (Out, Out) {
    // A tag per call: tests run in parallel, and each fixture's drop removes its file.
    static CALLS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let with = Fixture::rule(true, &format!("with-{}-{n}", args[0]));
    let without = Fixture::rule(false, &format!("without-{}-{n}", args[0]));
    let swap = |f: &Fixture| -> Vec<String> {
        args.iter()
            .map(|a| if *a == "PATH" { f.path() } else { a }.to_owned())
            .collect()
    };
    let w: Vec<String> = swap(&with);
    let r = run(&w.iter().map(String::as_str).collect::<Vec<_>>());
    assert_eq!(r.code, 0, "{args:?}: {}", r.stderr);
    assert!(r.stderr.contains(WARNING), "{args:?}: {}", r.stderr);
    assert!(r.stderr.contains(RULE_KIND), "{args:?}: {}", r.stderr);
    assert!(r.stderr.contains(consequence), "{args:?}: {}", r.stderr);
    assert!(
        !r.stderr.contains("sound under-approximation"),
        "{args:?}: not a sound under-approximation here: {}",
        r.stderr
    );
    let c: Vec<String> = swap(&without);
    let ctl = run(&c.iter().map(String::as_str).collect::<Vec<_>>());
    assert_eq!(ctl.code, 0, "{args:?} control: {}", ctl.stderr);
    assert!(
        !ctl.stderr.contains(WARNING),
        "{args:?} control: {}",
        ctl.stderr
    );
    (r, ctl)
}

fn json(out: &Out) -> serde_json::Value {
    serde_json::from_str(&out.stdout).unwrap_or_else(|e| panic!("{e}: {}", out.stdout))
}

#[test]
fn justify_warns_and_its_not_entailed_is_qualified() {
    let (r, ctl) = warns_on_the_drop_only(
        &["justify", "PATH", "subclass", A, B],
        "a justification that needs a dropped axiom is missing",
    );
    // The rule's entailment is what goes missing: "not entailed", now qualified.
    assert!(r.stdout.contains("not entailed"), "{}", r.stdout);
    assert!(ctl.stdout.contains("not entailed"), "{}", ctl.stdout);
}

#[test]
fn justify_json_carries_dropped_and_voids_enumeration_complete() {
    let (r, ctl) = warns_on_the_drop_only(
        &["justify", "--json", "PATH", "subclass", A, B],
        "a justification that needs a dropped axiom is missing",
    );
    let (j, c) = (json(&r), json(&ctl));
    assert_eq!(j["status"], "not-entailed", "{j}");
    assert_eq!(j["dropped"]["Rule: unsupported axiom (SWRL)"], 1, "{j}");
    assert_eq!(c["dropped"], serde_json::json!({}), "{c}");
    // The single-justification query on an entailed query: complete only without the
    // drop, too.
    let (r, ctl) = warns_on_the_drop_only(
        &["justify", "--json", "PATH", "subclass", A, C],
        "a justification that needs a dropped axiom is missing",
    );
    assert_eq!(json(&r)["enumeration_complete"], false, "{}", r.stdout);
    assert_eq!(json(&ctl)["enumeration_complete"], true, "{}", ctl.stdout);
    // `--all` on an entailed query: complete only without the drop.
    let (r, ctl) = warns_on_the_drop_only(
        &["justify", "--all", "--json", "PATH", "subclass", A, C],
        "a justification that needs a dropped axiom is missing",
    );
    let (j, c) = (json(&r), json(&ctl));
    assert_eq!(j["status"], "entailed", "{j}");
    assert_eq!(j["enumeration_complete"], false, "{j}");
    assert_eq!(c["enumeration_complete"], true, "{c}");
}

#[test]
fn prove_warns_and_its_json_carries_dropped() {
    warns_on_the_drop_only(
        &["prove", "PATH", A, B],
        "a proof that needs a dropped axiom is missing",
    );
    let (r, ctl) = warns_on_the_drop_only(
        &["prove", "--json", "PATH", A, B],
        "a proof that needs a dropped axiom is missing",
    );
    let (j, c) = (json(&r), json(&ctl));
    assert_eq!(j["status"], "not-entailed", "{j}");
    assert_eq!(j["dropped"]["Rule: unsupported axiom (SWRL)"], 1, "{j}");
    assert_eq!(c["dropped"], serde_json::json!({}), "{c}");
    // An entailed query keeps its proof; the block rides on it too.
    let (r, _) = warns_on_the_drop_only(
        &["prove", "--json", "PATH", A, C],
        "a proof that needs a dropped axiom is missing",
    );
    let j = json(&r);
    assert_eq!(j["status"], "entailed", "{j}");
    assert_eq!(j["dropped"]["Rule: unsupported axiom (SWRL)"], 1, "{j}");
}

#[test]
fn repair_warns_and_claims_completeness_over_the_converted_ontology_only() {
    let (r, ctl) = warns_on_the_drop_only(
        &["repair", "PATH", "subclass", A, C],
        "a repair may not break the entailment in the full ontology",
    );
    assert!(r.stderr.contains("may not be minimal"), "{}", r.stderr);
    assert!(
        r.stdout.contains("complete w.r.t. the converted ontology"),
        "{}",
        r.stdout
    );
    assert!(
        ctl.stdout.contains("minimal repair(s) — complete\n"),
        "{}",
        ctl.stdout
    );
}

#[test]
fn diagnose_warns() {
    let (r, ctl) = warns_on_the_drop_only(
        &["diagnose", "PATH"],
        "an unsatisfiable class or an inconsistency that needs a dropped axiom goes undetected",
    );
    assert!(
        r.stdout.contains("http://e#D"),
        "the unsat class is still shown: {}",
        r.stdout
    );
    // "consistent" holds over the converted ontology only.
    assert!(
        r.stdout
            .contains("# consistency: consistent (over the converted ontology"),
        "{}",
        r.stdout
    );
    assert!(
        ctl.stdout.contains("# consistency: consistent\n"),
        "{}",
        ctl.stdout
    );
}

#[test]
fn report_warns_and_its_html_shows_the_drop() {
    let (r, ctl) = warns_on_the_drop_only(
        &["report", "PATH"],
        "an unsatisfiable class or an inconsistency that needs a dropped axiom goes undetected",
    );
    assert!(
        r.stdout.contains("<strong>Axioms dropped:</strong>"),
        "{}",
        r.stdout
    );
    assert!(
        r.stdout.contains("Rule: unsupported axiom (SWRL) ×1"),
        "{}",
        r.stdout
    );
    assert!(
        r.stdout
            .contains("verified against the reasoner, over the converted ontology"),
        "{}",
        r.stdout
    );
    assert!(!ctl.stdout.contains("Axioms dropped:"), "{}", ctl.stdout);
    assert!(
        !ctl.stdout.contains("over the converted ontology"),
        "{}",
        ctl.stdout
    );
}

/// #543 through the report: a banner, not masked on an inconsistent ontology (its
/// repairs rest on negative answers). `owl:topObjectProperty` is reached through a
/// prefix that cuts inside the name, which the old byte prefilter could not see, so
/// the stderr note is pinned on the five commands' conversion-based flag as well.
#[test]
fn the_universal_role_shows_in_the_report_even_when_inconsistent() {
    let src = "Prefix(:=<http://e#>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Prefix(t:=<http://www.w3.org/2002/07/owl#top>)
Ontology(
Declaration(Class(:C)) Declaration(Class(:D)) Declaration(NamedIndividual(:a))
SubClassOf(:C ObjectSomeValuesFrom(t:ObjectProperty :D))
SubClassOf(:C ObjectAllValuesFrom(t:ObjectProperty :D))
ClassAssertion(owl:Nothing :a)
)";
    let f = Fixture::new(src, "uro-report");
    let r = run(&["report", f.path()]);
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert!(
        r.stdout.contains("<strong>Universal role:</strong>"),
        "{}",
        r.stdout
    );
    assert!(
        r.stderr.contains("owl:topObjectProperty is used here"),
        "{}",
        r.stderr
    );
    for args in [
        vec!["justify", f.path(), "inconsistent"],
        vec!["repair", f.path(), "inconsistent"],
        vec!["prove", f.path(), "http://e#C", "http://e#D"],
        vec!["diagnose", f.path()],
    ] {
        let r = run(&args);
        assert!(
            r.stderr.contains("owl:topObjectProperty is used here"),
            "{args:?}: {}",
            r.stderr
        );
    }
}

/// The role-level commands lost the same byte prefilter: a role above `top`, named
/// through a prefix that cuts inside the name, is flagged on `property-values`.
#[test]
fn a_role_above_top_through_a_cut_prefix_is_flagged_on_property_values() {
    let src = "Prefix(:=<http://e#>)
Prefix(t:=<http://www.w3.org/2002/07/owl#top>)
Ontology(
Declaration(ObjectProperty(:r)) Declaration(NamedIndividual(:a))
SubObjectPropertyOf(t:ObjectProperty :r)
)";
    let f = Fixture::new(src, "uro-roles");
    let r = run(&["property-values", "--json", f.path()]);
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert_eq!(json(&r)["universal_role_ordinary"], true, "{}", r.stdout);
    assert!(
        r.stderr.contains("owl:topObjectProperty is used here"),
        "{}",
        r.stderr
    );
}
