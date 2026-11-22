//! #540: the per-query surfaces must not claim completeness when conversion
//! dropped an axiom. `xsd:dateTimeStamp` is an unsupported data range, so each
//! axiom that mentions it is dropped and the rest is pure EL. Before the fix
//! `subclass-expr` / `instances-expr` / `sat-expr --json` reported
//! `completeness_guaranteed: true` and `explain` said "closure is complete"
//! while missing an entailment of the dropped axioms.
//!
//! Every case has a control: the same file with `xsd:string` (supported) drops
//! nothing, derives the entailment and keeps the guarantee, so the drop is the
//! only thing the negative case varies.
#![allow(clippy::unwrap_used)]

use std::path::PathBuf;
use std::process::Command;

const HEAD: &str = "Prefix(:=<http://e#>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)
Ontology(
Declaration(Class(:C)) Declaration(Class(:E)) Declaration(DataProperty(:p))
";

/// `C ⊑ ∃p.R ⊑ E`: `C ⊑ E` for every data range `R`.
const SUBSUMPTION: &str = "SubClassOf(:C DataSomeValuesFrom(:p RANGE))
SubClassOf(DataSomeValuesFrom(:p RANGE) :E)
)";

/// As `SUBSUMPTION` plus `C(a)`: `a` is an instance of `E`.
const INSTANCE: &str = "Declaration(NamedIndividual(:a))
SubClassOf(:C DataSomeValuesFrom(:p RANGE))
SubClassOf(DataSomeValuesFrom(:p RANGE) :E)
ClassAssertion(:C :a)
)";

/// `C ⊑ ∃p.R ⊑ ⊥`: `C` is unsatisfiable. An EL-only sat probe.
const UNSAT: &str = "SubClassOf(:C DataSomeValuesFrom(:p RANGE))
SubClassOf(DataSomeValuesFrom(:p RANGE) owl:Nothing)
)";

const DROPPED: &str = "xsd:dateTimeStamp";
const KEPT: &str = "xsd:string";

struct Fixture(PathBuf);

impl Fixture {
    fn new(body: &str, range: &str, tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "rustdl-expr-dropped-{}-{tag}.ofn",
            std::process::id()
        ));
        std::fs::write(&path, format!("{HEAD}{}", body.replace("RANGE", range))).unwrap();
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

fn assert_drop_warned(stderr: &str, what: &str) {
    assert!(
        stderr.contains("not understood and dropped"),
        "{what}: precondition, the axioms are dropped: {stderr}"
    );
}

#[test]
fn subclass_expr_with_a_dropped_axiom_is_not_guaranteed() {
    let f = Fixture::new(SUBSUMPTION, DROPPED, "sub-drop");
    let (v, stderr) = json(&["subclass-expr", "--json", f.path(), ":C", ":E"]);
    assert_drop_warned(&stderr, "subclass-expr");
    assert_eq!(v["entailed"], false, "precondition: C ⊑ E is missed: {v}");
    assert_eq!(v["completeness_guaranteed"], false, "{v}");

    let c = Fixture::new(SUBSUMPTION, KEPT, "sub-keep");
    let (v, _) = json(&["subclass-expr", "--json", c.path(), ":C", ":E"]);
    assert_eq!(v["entailed"], true, "control: {v}");
    assert_eq!(v["completeness_guaranteed"], true, "control: {v}");
}

#[test]
fn instances_expr_with_a_dropped_axiom_is_not_guaranteed() {
    let f = Fixture::new(INSTANCE, DROPPED, "inst-drop");
    let (v, stderr) = json(&["instances-expr", "--json", f.path(), ":E"]);
    assert_drop_warned(&stderr, "instances-expr");
    assert_eq!(v["instances"], serde_json::json!([]), "precondition: {v}");
    assert_eq!(v["completeness_guaranteed"], false, "{v}");

    let c = Fixture::new(INSTANCE, KEPT, "inst-keep");
    let (v, _) = json(&["instances-expr", "--json", c.path(), ":E"]);
    assert_eq!(
        v["instances"],
        serde_json::json!(["http://e#a"]),
        "control: {v}"
    );
    assert_eq!(v["completeness_guaranteed"], true, "control: {v}");
}

#[test]
fn el_only_sat_expr_with_a_dropped_axiom_is_not_guaranteed() {
    let f = Fixture::new(UNSAT, DROPPED, "sat-drop");
    let (v, stderr) = json(&["sat-expr", "--json", f.path(), ":C"]);
    assert_drop_warned(&stderr, "sat-expr");
    assert_eq!(
        v["satisfiable"], true,
        "precondition: C's unsat is missed: {v}"
    );
    assert_eq!(v["completeness_guaranteed"], false, "{v}");

    let c = Fixture::new(UNSAT, KEPT, "sat-keep");
    let (v, _) = json(&["sat-expr", "--json", c.path(), ":C"]);
    assert_eq!(v["satisfiable"], false, "control: {v}");
    // The control is on the pure-EL fast path, so the negative case above is too
    // (the drop is the only difference).
    assert_eq!(v["completeness_guaranteed"], true, "control: {v}");
}

#[test]
fn explain_with_a_dropped_axiom_does_not_claim_a_complete_closure() {
    let f = Fixture::new(SUBSUMPTION, DROPPED, "explain-drop");
    let r = run(&["explain", f.path(), "http://e#C", "http://e#E"]);
    assert_drop_warned(&r.stderr, "explain");
    assert!(
        r.stdout.contains(": no "),
        "precondition: C ⊑ E is missed: {}",
        r.stdout
    );
    assert!(
        !r.stdout.contains("closure is complete"),
        "a dropped axiom: explain must not claim completeness: {}",
        r.stdout
    );
    assert!(r.stdout.contains("NOT complete"), "{}", r.stdout);

    // Control: no drop, a pure-EL "no" (E ⋢ C) keeps the claim.
    let c = Fixture::new(SUBSUMPTION, KEPT, "explain-keep");
    let r = run(&["explain", c.path(), "http://e#E", "http://e#C"]);
    assert!(r.stdout.contains(": no "), "control: {}", r.stdout);
    assert!(
        r.stdout.contains("closure is complete"),
        "control: {}",
        r.stdout
    );
    assert!(!r.stderr.contains("dropped"), "control: {}", r.stderr);
}

// --- Query-side drops (#540 review) ---------------------------------------
//
// The ontology itself drops nothing; the QUERY EXPRESSION holds the unsupported
// range, so only the probe axiom `Q ≡ ce` is dropped. The flag must read false
// and the stderr warning must name the drop in both text and JSON modes; before
// the review fix the warning came from the unprobed ontology and said nothing.
// Each control swaps the range for `xsd:string`.

/// Pure EL and ABox-free (so `sat-expr` / `subclass-expr` take the pure-EL fast
/// path): `C ⊑ ∃p.string`, `D ⊑ ⊥`.
const CLEAN_TBOX: &str = "Declaration(Class(:D))
SubClassOf(:C DataSomeValuesFrom(:p xsd:string))
SubClassOf(:D owl:Nothing)
)";

/// As `CLEAN_TBOX` plus `C(a)`, for the instance query.
const CLEAN: &str = "Declaration(Class(:D)) Declaration(NamedIndividual(:a))
SubClassOf(:C DataSomeValuesFrom(:p xsd:string))
SubClassOf(:D owl:Nothing)
ClassAssertion(:C :a)
)";

/// Runs `args` in JSON mode then in text mode; returns the JSON and each stderr.
fn both_modes(cmd: &str, file: &str, ces: &[&str]) -> (serde_json::Value, String, String) {
    let mut args = vec![cmd, "--json", file];
    args.extend_from_slice(ces);
    let (v, json_err) = json(&args);
    args.remove(1);
    let text = run(&args);
    (v, json_err, text.stderr)
}

fn assert_query_drop(json_err: &str, text_err: &str, what: &str) {
    for (mode, err) in [("json", json_err), ("text", text_err)] {
        assert!(
            err.contains("not understood and dropped"),
            "{what} ({mode}): a query-side drop must be warned: {err:?}"
        );
    }
}

fn assert_no_drop(json_err: &str, text_err: &str, what: &str) {
    for (mode, err) in [("json", json_err), ("text", text_err)] {
        assert!(!err.contains("dropped"), "{what} control ({mode}): {err:?}");
    }
}

#[test]
fn subclass_expr_with_a_query_side_drop_warns_and_is_not_guaranteed() {
    let f = Fixture::new(CLEAN_TBOX, KEPT, "q-sub");
    // X ⊑ X, entailed for any X; the dropped probe axioms leave it underived.
    let x = format!(":p some {DROPPED}");
    let (v, je, te) = both_modes("subclass-expr", f.path(), &[&x, &x]);
    assert_query_drop(&je, &te, "subclass-expr");
    assert_eq!(v["entailed"], false, "precondition: X ⊑ X is missed: {v}");
    assert_eq!(v["completeness_guaranteed"], false, "{v}");

    let x = format!(":p some {KEPT}");
    let (v, je, te) = both_modes("subclass-expr", f.path(), &[&x, &x]);
    assert_no_drop(&je, &te, "subclass-expr");
    assert_eq!(v["entailed"], true, "control: {v}");
    assert_eq!(v["completeness_guaranteed"], true, "control: {v}");
}

#[test]
fn sat_expr_with_a_query_side_drop_warns_and_is_not_guaranteed() {
    let f = Fixture::new(CLEAN_TBOX, KEPT, "q-sat");
    // D ⊑ ⊥, so D ⊓ X is unsatisfiable for any X. Conversion keeps the `D`
    // conjunct and still finds it; the flag must read false all the same, since
    // the dropped part could matter (the guarantee is conservative).
    let q = format!(":D and (:p some {DROPPED})");
    let (v, je, te) = both_modes("sat-expr", f.path(), &[&q]);
    assert_query_drop(&je, &te, "sat-expr");
    assert_eq!(v["completeness_guaranteed"], false, "{v}");

    let q = format!(":D and (:p some {KEPT})");
    let (v, je, te) = both_modes("sat-expr", f.path(), &[&q]);
    assert_no_drop(&je, &te, "sat-expr");
    assert_eq!(v["satisfiable"], false, "control: {v}");
    assert_eq!(v["completeness_guaranteed"], true, "control: {v}");
}

#[test]
fn instances_expr_with_a_query_side_drop_warns_and_is_not_guaranteed() {
    let f = Fixture::new(CLEAN, KEPT, "q-inst");
    let q = format!(":C and (:p some {DROPPED})");
    let (v, je, te) = both_modes("instances-expr", f.path(), &[&q]);
    assert_query_drop(&je, &te, "instances-expr");
    assert_eq!(v["completeness_guaranteed"], false, "{v}");

    let q = format!(":C and (:p some {KEPT})");
    let (v, je, te) = both_modes("instances-expr", f.path(), &[&q]);
    assert_no_drop(&je, &te, "instances-expr");
    assert_eq!(
        v["instances"],
        serde_json::json!(["http://e#a"]),
        "control: {v}"
    );
    assert_eq!(v["completeness_guaranteed"], true, "control: {v}");
}
