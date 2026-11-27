//! #584 item 3: `justify … inconsistent` (and `subclass owl:Thing owl:Nothing`, which
//! is canonicalised to it) switches every subset probe to the saturation-only
//! consistency check when the full set's inconsistency is saturation-detected. That
//! check is a sound refuter, but its "not refuted" is no consistency proof (the `ABox`
//! saturator has no anonymous witnesses and no `∀` rule), so `QuickXplain` can keep an
//! axiom a smaller subset does without. The result is valid but was reported
//! `minimal: true`: on `H2` it returned 7 axioms while 5 of them are inconsistent.
//! Found by the Fable plan review of #584 (both shapes are labelled `Horn`; `EL` uses
//! EL constructs only, and is `Horn` only because any `ABox` leaves pure EL).
//!
//! Each case pins the default (`minimal: false`) and, as the control arm,
//! `RUSTDL_JUSTIFY_SAT_ORACLE=0` (the full oracle: the minimal set, `minimal: true`).
#![allow(clippy::unwrap_used)]
#![allow(clippy::doc_markdown)]

use std::path::PathBuf;
use std::process::Command;

const HEAD: &str = "Prefix(:=<http://ex.org/>)\nOntology(<http://ex.org/o>\n\
    Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C))\n\
    Declaration(Class(:D)) Declaration(Class(:E))\n\
    Declaration(ObjectProperty(:r)) Declaration(ObjectProperty(:q))\n\
    Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))\n";

/// `A(a)`, `A ⊑ D ⊓ ∀r.B`, `r(a,b)`, `range(r) = C`, `B ⊓ C ⊑ ⊥` is inconsistent on its
/// own; `D ⊑ ∃q.{b}` and `range(q) = B` add a second, saturation-visible route.
const H2: &str = "ClassAssertion(:A :a)\n\
    SubClassOf(:A ObjectIntersectionOf(:D ObjectAllValuesFrom(:r :B)))\n\
    ObjectPropertyAssertion(:r :a :b)\n\
    ObjectPropertyRange(:r :C)\n\
    DisjointClasses(:B :C)\n\
    SubClassOf(:D ObjectHasValue(:q :b))\n\
    ObjectPropertyRange(:q :B)\n";

/// `A(a)`, `A ⊑ E ⊓ ∃r.B`, `domain(r) = C`, `C ⊓ E ⊑ ⊥` is inconsistent (through the
/// anonymous `r`-successor); `r(a,b)` adds the saturation-visible route.
const EL: &str = "ClassAssertion(:A :a)\n\
    SubClassOf(:A ObjectIntersectionOf(:E ObjectSomeValuesFrom(:r :B)))\n\
    ObjectPropertyDomain(:r :C)\n\
    DisjointClasses(:C :E)\n\
    ObjectPropertyAssertion(:r :a :b)\n";

struct Fixture(PathBuf);

impl Fixture {
    fn new(body: &str, tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "rustdl-justify-sat-oracle-{}-{tag}.ofn",
            std::process::id()
        ));
        std::fs::write(&path, format!("{HEAD}{body})\n")).unwrap();
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

fn json(args: &[&str], env: &[(&str, &str)]) -> serde_json::Value {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_rustdl"));
    cmd.args(args);
    // The default arm must be the default even if the caller's shell set it.
    cmd.env_remove("RUSTDL_JUSTIFY_SAT_ORACLE");
    for (k, v) in env {
        cmd.env(k, v);
    }
    let out = cmd.output().unwrap();
    assert!(out.status.success(), "{args:?} {env:?}: {out:?}");
    serde_json::from_slice(&out.stdout).unwrap()
}

const FULL_ORACLE: (&str, &str) = ("RUSTDL_JUSTIFY_SAT_ORACLE", "0");
const THING: &str = "http://www.w3.org/2002/07/owl#Thing";
const NOTHING: &str = "http://www.w3.org/2002/07/owl#Nothing";

/// `(axiom count, minimal)` of the single justification. Its `ofn` prints one axiom
/// per line (the first shares the `Ontology(` line); `minimal` is top-level.
fn justification(v: &serde_json::Value) -> (usize, bool) {
    let js = v["justifications"].as_array().unwrap();
    assert_eq!(js.len(), 1, "{v}");
    let ofn = js[0]["ofn"].as_str().unwrap();
    let axioms = ofn
        .lines()
        .filter(|l| !l.starts_with("Prefix(") && l.trim() != ")")
        .filter(|l| l.trim_start_matches("Ontology(").trim().contains('('))
        .count();
    (axioms, v["minimal"].as_bool().unwrap())
}

#[test]
fn the_saturation_oracle_does_not_claim_minimality() {
    for (tag, body, minimal_size) in [("h2", H2, 5), ("el", EL, 4)] {
        let f = Fixture::new(body, tag);
        for query in [
            vec!["justify", "--json", f.path(), "inconsistent"],
            vec!["justify", "--json", f.path(), "subclass", THING, NOTHING],
            vec!["justify", "--json", f.path(), "unsat", THING],
        ] {
            let (n, minimal) = justification(&json(&query, &[]));
            assert!(!minimal, "{tag} {query:?}: {n} axioms claimed minimal");
            assert!(
                n > minimal_size,
                "{tag}: precondition, the set is not minimal ({n})"
            );
            let (n, minimal) = justification(&json(&query, &[FULL_ORACLE]));
            assert_eq!(
                (n, minimal),
                (minimal_size, true),
                "{tag} {query:?}: full oracle"
            );
        }
        // `--all` runs the HST search (`search_all`), the other site.
        let all = ["justify", "--json", "--all", f.path(), "inconsistent"];
        assert_eq!(json(&all, &[])["minimal"], false, "{tag} --all");
        assert_eq!(
            json(&all, &[FULL_ORACLE])["minimal"],
            true,
            "{tag} --all: full oracle"
        );
    }
}

/// `prove` inherits it: on an inconsistent KB its justification is the inconsistency
/// core (#456), and `justification_minimal` read the false claim.
#[test]
fn prove_inherits_the_cleared_claim() {
    let f = Fixture::new(H2, "prove");
    let args = [
        "prove",
        "--json",
        f.path(),
        "http://ex.org/A",
        "http://ex.org/E",
    ];
    let v = json(&args, &[]);
    assert_eq!(v["kb_inconsistent"], true, "{v}");
    assert_eq!(v["justification_minimal"], false, "{v}");
    let v = json(&args, &[FULL_ORACLE]);
    assert_eq!(v["justification_minimal"], true, "full oracle: {v}");
}

/// #584 review: under `--all`, a branch probe on the saturation-only oracle prunes on
/// "not refuted", so a justification can be missing while `enumeration_complete` read
/// `true` (Protégé reads it as "all explanations found"). `TWO` has two independent
/// clashes: the `∃r`/domain one the saturator misses, and `X ⊓ Y` on `c`, which it
/// detects; the default search finds only the second.
const TWO: &str = "ClassAssertion(:A :a)\n\
    SubClassOf(:A ObjectIntersectionOf(:E ObjectSomeValuesFrom(:r :B)))\n\
    ObjectPropertyDomain(:r :C)\n\
    DisjointClasses(:C :E)\n\
    Declaration(Class(:X)) Declaration(Class(:Y)) Declaration(NamedIndividual(:c))\n\
    ClassAssertion(:X :c) ClassAssertion(:Y :c) DisjointClasses(:X :Y)\n";

#[test]
fn the_saturation_oracle_does_not_claim_a_complete_enumeration() {
    let f = Fixture::new(TWO, "two");
    let args = ["justify", "--json", "--all", f.path(), "inconsistent"];
    let v = json(&args, &[]);
    assert_eq!(v["enumeration_complete"], false, "{v}");
    assert_eq!(
        v["justifications"].as_array().unwrap().len(),
        1,
        "precondition: {v}"
    );
    let v = json(&args, &[FULL_ORACLE]);
    assert_eq!(v["enumeration_complete"], true, "full oracle: {v}");
    assert_eq!(
        v["justifications"].as_array().unwrap().len(),
        2,
        "full oracle: {v}"
    );
}

/// `repair` inherits it: its `complete` needs every justification minimal, and it now
/// says so on the empty-repairs path too.
#[test]
fn repair_does_not_claim_completeness_on_the_saturation_oracle() {
    let f = Fixture::new(H2, "repair");
    let text = |env: &[(&str, &str)]| {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_rustdl"));
        cmd.args(["repair", f.path(), "inconsistent"]);
        cmd.env_remove("RUSTDL_JUSTIFY_SAT_ORACLE");
        for (k, v) in env {
            cmd.env(k, v);
        }
        let out = cmd.output().unwrap();
        assert!(out.status.success(), "{out:?}");
        String::from_utf8_lossy(&out.stdout).into_owned()
    };
    let t = text(&[]);
    assert!(t.contains("completeness not guaranteed"), "{t}");
    let t = text(&[FULL_ORACLE]);
    assert!(t.contains("— complete"), "full oracle: {t}");
}
