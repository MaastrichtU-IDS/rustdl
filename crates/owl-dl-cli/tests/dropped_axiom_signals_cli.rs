//! #579: two holes of the #557 kind.
//!
//! 1. An unresolved `Import` was skipped as metadata (`Ok(None)`), so a pure-EL file
//!    importing anything read `completeness_guaranteed: true`, `dropped: {}`, no
//!    warning, and `verify-el` said `verified`. Nothing resolves imports, so the
//!    imported axioms are missing: it is now counted in `dropped`.
//! 2. The property and individual surfaces did not read `dropped`:
//!    `property-values` claimed `incomplete: false` (its documented guarantee)
//!    while missing the edge a SWRL rule entails, the data half and
//!    `property-hierarchy` / `disjoint` reported no structural gap, and
//!    `individuals` claimed its seed complete. None of the four JSONs had a
//!    `dropped` block.
//!
//! Every case has a control without the dropped axiom.
#![allow(clippy::unwrap_used)]
#![allow(clippy::doc_markdown)]

use std::path::PathBuf;
use std::process::Command;

const IMPORT_KIND: &str = "Import: unsupported axiom (unresolved import)";
const RULE_KIND: &str = "Rule: unsupported axiom (SWRL)";
const IMPORT: &str = "Import(<http://example.org/not-loaded>)";

struct Fixture(PathBuf);

impl Fixture {
    /// `head` goes before the axioms (where `Import` must be), `body` after.
    fn new(head: &str, body: &str, tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "rustdl-dropped-signals-{}-{tag}.ofn",
            std::process::id()
        ));
        std::fs::write(
            &path,
            format!(
                "Prefix(:=<http://e#>)\nPrefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)\n\
                 Ontology(<http://e/o>\n{head}\n\
                 Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C))\n\
                 Declaration(ObjectProperty(:r)) Declaration(ObjectProperty(:s))\n\
                 Declaration(DataProperty(:d))\n\
                 Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))\n\
                 Declaration(NamedIndividual(:c)) Declaration(NamedIndividual(:e))\n\
                 {body}\n)"
            ),
        )
        .unwrap();
        Self(path)
    }

    /// A whole ontology document, for fixtures that must declare no individual.
    fn raw(text: &str, tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "rustdl-dropped-signals-{}-{tag}.ofn",
            std::process::id()
        ));
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

fn run(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_rustdl"))
        .args(args)
        .output()
        .unwrap()
}

fn json(args: &[&str]) -> serde_json::Value {
    let out = run(args);
    assert!(out.status.success(), "{args:?}: {out:?}");
    serde_json::from_slice(&out.stdout).unwrap()
}

const EL: &str = "SubClassOf(:A :C)";

#[test]
fn an_unresolved_import_withdraws_every_guarantee() {
    let f = Fixture::new(IMPORT, EL, "import-el");
    let c = Fixture::new("", EL, "import-el-control");

    let v = json(&["classify", "--json", f.path()]);
    assert_eq!(v["dropped"][IMPORT_KIND], 1, "{v}");
    assert_eq!(v["completeness_guaranteed"], false, "{v}");
    let v = json(&["classify", "--saturation-only", "--json", f.path()]);
    assert_eq!(v["completeness_guaranteed"], false, "saturation-only: {v}");
    let err = String::from_utf8_lossy(&run(&["classify", f.path()]).stderr).into_owned();
    assert!(err.contains(IMPORT_KIND), "the CLI must warn: {err}");
    for args in [
        ["sat-expr", "--json", f.path(), ":A"],
        ["instances-expr", "--json", f.path(), ":A"],
    ] {
        assert_eq!(json(&args)["completeness_guaranteed"], false, "{args:?}");
    }
    // A "no" on pure EL must not claim the closure is complete.
    let out = run(&["explain", f.path(), "http://e#C", "http://e#A"]);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(!text.contains("closure is complete"), "{text}");
    // `verify-el` cannot verify a closure of a KB it did not see (exit 3).
    assert_eq!(run(&["verify-el", f.path()]).status.code(), Some(3));

    let v = json(&["classify", "--json", c.path()]);
    assert_eq!(v["dropped"], serde_json::json!({}), "control: {v}");
    assert_eq!(v["completeness_guaranteed"], true, "control: {v}");
    for args in [
        ["sat-expr", "--json", c.path(), ":A"],
        ["instances-expr", "--json", c.path(), ":A"],
    ] {
        assert_eq!(
            json(&args)["completeness_guaranteed"],
            true,
            "control {args:?}"
        );
    }
    let out = run(&["explain", c.path(), "http://e#C", "http://e#A"]);
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("closure is complete"),
        "control: {out:?}"
    );
    assert_eq!(run(&["verify-el", c.path()]).status.code(), Some(0));
}

/// `A(?x), B(?y) -> r(?x,?y)` with `A(a)`, `B(b)` entails `r(a,b)` (HermiT). No seed
/// edge, so no extension probe runs, and every converted axiom is whitelisted: before
/// #579 `incomplete` read false, the documented guarantee, while `r(a,b)` was missing.
#[test]
fn property_values_reads_dropped() {
    let rule = "DLSafeRule(Body(ClassAtom(:A Variable(<urn:x>)) ClassAtom(:B Variable(<urn:y>))) \
                Head(ObjectPropertyAtom(:r Variable(<urn:x>) Variable(<urn:y>))))";
    let abox = "ClassAssertion(:A :a) ClassAssertion(:B :b)";
    for (tag, head, body, kind) in [
        ("rule", "", format!("{abox} {rule}"), RULE_KIND),
        ("import", IMPORT, abox.to_string(), IMPORT_KIND),
    ] {
        let f = Fixture::new(head, &body, &format!("pv-{tag}"));
        let v = json(&["property-values", "--json", f.path()]);
        assert_eq!(
            v["object_property_values"],
            serde_json::json!([]),
            "{tag}: {v}"
        );
        assert_eq!(v["incomplete"], true, "{tag}: {v}");
        assert_eq!(v["dropped"][kind], 1, "{tag}: {v}");
        assert!(
            v["structural_gaps"]
                .as_array()
                .unwrap()
                .contains(&serde_json::json!("data-dropped-axioms")),
            "{tag}: {v}"
        );
    }
    let c = Fixture::new("", abox, "pv-control");
    let v = json(&["property-values", "--json", c.path()]);
    assert_eq!(v["incomplete"], false, "control: {v}");
    assert_eq!(v["dropped"], serde_json::json!({}), "control: {v}");
    assert_eq!(v["structural_gaps"], serde_json::json!([]), "control: {v}");
}

/// The data half: `A(?x) -> d(?x, "v")` with `A(a)` entails `d(a, "v")`, which the
/// passthrough misses. It reports the drop as the gap `data-dropped-axioms`.
#[test]
fn data_property_values_report_the_drop_as_a_gap() {
    let f = Fixture::new(
        "",
        "ClassAssertion(:A :a) DLSafeRule(Body(ClassAtom(:A Variable(<urn:x>))) \
         Head(DataPropertyAtom(:d Variable(<urn:x>) \"v\")))",
        "pv-data-rule",
    );
    let v = json(&["property-values", "--json", f.path()]);
    assert_eq!(v["data_property_values"], serde_json::json!([]), "{v}");
    assert_eq!(
        v["structural_gaps"],
        serde_json::json!(["data-dropped-axioms"]),
        "{v}"
    );
    let err = String::from_utf8_lossy(&run(&["property-values", f.path()]).stderr).into_owned();
    assert!(err.contains("data-dropped-axioms"), "{err}");
}

/// `-> sameAs(c, e)` (an empty body) entails `c = e`. No individual is declared
/// and the rule's are never interned, so no probe runs and nothing else sets
/// `incomplete`: before #579 it read `false` while missing the equality.
/// `same_individuals`' seed is complete only for converted axioms.
#[test]
fn individuals_reads_dropped() {
    let doc = |extra: &str| {
        format!(
            "Prefix(:=<http://e#>)\nOntology(<http://e/o>\n\
             Declaration(Class(:A)) SubClassOf(:A :A)\n{extra}\n)"
        )
    };
    let f = Fixture::raw(
        &doc("DLSafeRule(Body() Head(SameIndividualAtom(:c :e)))"),
        "ind-rule",
    );
    let v = json(&["individuals", "--json", f.path()]);
    assert_eq!(v["same_groups"], serde_json::json!([]), "{v}");
    assert_eq!(v["incomplete"], true, "{v}");
    assert_eq!(v["dropped"][RULE_KIND], 1, "{v}");
    let c = Fixture::raw(&doc(""), "ind-control");
    let v = json(&["individuals", "--json", c.path()]);
    assert_eq!(v["incomplete"], false, "control: {v}");
    assert_eq!(v["dropped"], serde_json::json!({}), "control: {v}");
}

/// `r(?x,?y) -> s(?x,?y)` entails `r ⊑ s`, which the structural hierarchy misses;
/// both property surfaces report the drop as a gap and carry `dropped`.
#[test]
fn property_hierarchy_and_disjoint_report_the_drop() {
    let rule = "DLSafeRule(Body(ObjectPropertyAtom(:r Variable(<urn:x>) Variable(<urn:y>))) \
                Head(ObjectPropertyAtom(:s Variable(<urn:x>) Variable(<urn:y>))))";
    let f = Fixture::new("", rule, "hier-rule");
    let v = json(&["property-hierarchy", "--json", f.path()]);
    assert!(
        v["structural_gaps"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!("object-dropped-axioms")),
        "{v}"
    );
    assert_eq!(v["dropped"][RULE_KIND], 1, "{v}");
    let v = json(&["disjoint", "--json", f.path()]);
    assert!(
        v["structural_gaps"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!("object-dropped-axioms")),
        "{v}"
    );
    assert_eq!(v["dropped"][RULE_KIND], 1, "{v}");

    let c = Fixture::new("", "SubObjectPropertyOf(:r :s)", "hier-control");
    for cmd in ["property-hierarchy", "disjoint"] {
        let v = json(&[cmd, "--json", c.path()]);
        assert_eq!(
            v["structural_gaps"],
            serde_json::json!([]),
            "control {cmd}: {v}"
        );
        assert_eq!(v["dropped"], serde_json::json!({}), "control {cmd}: {v}");
    }
}
