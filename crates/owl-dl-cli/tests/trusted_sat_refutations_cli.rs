//! #568: `classify --json`'s `trusted_sat_refutations` counts every verdict taken
//! on a trusted wedge `Sat`, in every fragment but PureEl (where no wedge runs).
//! Before, it exempted `Horn` and left out the label-cache prunes and the classes
//! the unsat probe took as satisfiable from their wedge label, so on the #564 shape
//! with `RUSTDL_CLASSIFY_WEDGE_IF_BOUND=0` it read 0 while `classify` called an
//! unsatisfiable class satisfiable.
//!
//! The per-pair budget is generous (`--pair-timeout-ms 2000`): at the CLI's 5 ms
//! default a loaded host can cut a label build and move a class to the tableau.
#![allow(clippy::unwrap_used)]

use std::path::PathBuf;
use std::process::Command;

struct Fixture(PathBuf);

impl Fixture {
    fn new(body: &str, tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "rustdl-trusted-sat-{}-{tag}.ofn",
            std::process::id()
        ));
        std::fs::write(
            &path,
            format!(
                "Prefix(:=<http://x.org/f#>)\n\
                 Prefix(owl:=<http://www.w3.org/2002/07/owl#>)\n\
                 Ontology(<http://x.org/f>\n{body}\n)"
            ),
        )
        .unwrap();
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

/// `classify --json` with `env` set; returns the JSON.
fn classify(file: &str, env: &[(&str, &str)]) -> serde_json::Value {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_rustdl"));
    cmd.args(["classify", "--json", "--pair-timeout-ms", "2000", file]);
    for (k, v) in env {
        cmd.env(k, v);
    }
    let out = cmd.output().unwrap();
    assert!(out.status.success(), "{out:?}");
    serde_json::from_slice(&out.stdout).unwrap()
}

/// The #564 reproducer: `Q` is unsatisfiable (HermiT; `rustdl sat Q`).
const REPRO: &str = "Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:Q))
Declaration(ObjectProperty(:r)) Declaration(ObjectProperty(:s)) Declaration(ObjectProperty(:t))
Declaration(NamedIndividual(:a))
SubClassOf(:Q ObjectMinCardinality(2 :r :A))
SubClassOf(:A ObjectSomeValuesFrom(:t :B))
SubClassOf(:B ObjectSomeValuesFrom(ObjectInverseOf(:s) ObjectOneOf(:a)))
FunctionalObjectProperty(:s)
InverseFunctionalObjectProperty(:t)";

#[test]
fn the_564_miss_is_not_reported_clean() {
    let f = Fixture::new(REPRO, "repro");
    let v = classify(f.path(), &[("RUSTDL_CLASSIFY_WEDGE_IF_BOUND", "0")]);
    assert_eq!(
        v["unsatisfiable"],
        serde_json::json!([]),
        "precondition: {v}"
    );
    assert_eq!(v["incomplete"], false, "precondition: {v}");
    // Each of the 3 classes was taken as satisfiable on its wedge label, and
    // pairs were pruned from those labels.
    let n = v["trusted_sat_refutations"].as_u64().unwrap();
    assert!(n > 3, "{v}");
    assert_eq!(v["completeness_guaranteed"], false, "{v}");

    // Default (the bound on): `Q` is found unsatisfiable by the wedge's `Unsat`,
    // and the other classes still rest on its `Sat`.
    let v = classify(f.path(), &[]);
    assert_eq!(
        v["unsatisfiable"],
        serde_json::json!(["http://x.org/f#Q"]),
        "{v}"
    );
    assert!(v["trusted_sat_refutations"].as_u64().unwrap() >= 2, "{v}");
}

/// One class, Horn but not EL, so no pair exists to prune or refute: the only
/// trusted verdict is the unsat probe's wedge `Sat` for `Q`. Pins that channel on
/// its own.
#[test]
fn a_class_taken_as_satisfiable_on_a_wedge_sat_is_counted() {
    let f = Fixture::new(
        "Declaration(Class(:Q)) Declaration(ObjectProperty(:r))\n\
         SubClassOf(:Q ObjectAllValuesFrom(:r :Q))",
        "one-class",
    );
    let v = classify(f.path(), &[]);
    assert_eq!(v["trusted_sat_refutations"], 1, "{v}");
    // Control: with the label path off the probe goes to the main tableau.
    let v = classify(f.path(), &[("RUSTDL_UNSAT_VIA_LABELS", "0")]);
    assert_eq!(v["trusted_sat_refutations"], 0, "control: {v}");
}

/// PureEl: answered by the saturator, so 0 without any fragment exemption.
#[test]
fn pure_el_reads_zero() {
    let f = Fixture::new(
        "Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:Q))
         Declaration(ObjectProperty(:r))
         SubClassOf(:A :B) SubClassOf(:Q ObjectSomeValuesFrom(:r :A))",
        "pure-el",
    );
    let v = classify(f.path(), &[]);
    assert_eq!(v["trusted_sat_refutations"], 0, "{v}");
    assert_eq!(v["completeness_guaranteed"], true, "{v}");
}
