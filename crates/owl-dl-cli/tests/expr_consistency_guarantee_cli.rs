//! #552: `instances-expr`'s completeness guarantee needs the KB's consistency to
//! have been checked. The realize fast path decides it eagerly
//! (`instance_query_inconsistent_fast`) unless `RUSTDL_QUERY_INCONSISTENCY_GUARD=0`,
//! and with the guard off it used to answer an inconsistent KB with a short list
//! and `completeness_guaranteed: true` (on an inconsistent KB every individual is
//! an instance).
#![allow(clippy::unwrap_used)]

use std::path::PathBuf;
use std::process::Command;

struct Fixture(PathBuf);

impl Fixture {
    fn new(body: &str, tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "rustdl-expr-consistency-{}-{tag}.ofn",
            std::process::id()
        ));
        std::fs::write(
            &path,
            format!(
                "Prefix(:=<http://e#>)\nPrefix(owl:=<http://www.w3.org/2002/07/owl#>)\n\
                 Ontology(\nDeclaration(Class(:A)) Declaration(Class(:B))\n\
                 Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))\n{body}\n)"
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

fn instances(file: &str, guard_off: bool) -> std::process::Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_rustdl"));
    cmd.args(["instances-expr", "--json", file, ":B"]);
    if guard_off {
        cmd.env("RUSTDL_QUERY_INCONSISTENCY_GUARD", "0");
    }
    cmd.output().unwrap()
}

fn json(out: &std::process::Output) -> serde_json::Value {
    assert!(out.status.success(), "{out:?}");
    serde_json::from_slice(&out.stdout).unwrap()
}

/// `a : A ⊑ ⊥` makes the KB inconsistent; `b : B` is the only told member of `B`.
const INCONSISTENT: &str = "ClassAssertion(:A :a) SubClassOf(:A owl:Nothing) ClassAssertion(:B :b)";
const CONSISTENT: &str = "ClassAssertion(:A :a) ClassAssertion(:B :b)";

#[test]
fn the_guard_off_fast_path_is_not_guaranteed() {
    let bad = Fixture::new(INCONSISTENT, "bad");
    // Default: the eager check finds the clash, and the query does not answer.
    assert!(
        !instances(bad.path(), false).status.success(),
        "precondition: the KB is inconsistent"
    );
    // Guard off: no consistency check ran, so the guarantee must not hold.
    let v = json(&instances(bad.path(), true));
    assert_eq!(v["completeness_guaranteed"], false, "{v}");

    // Control: a consistent KB on the fast path keeps the guarantee with the guard
    // on; with it off, it is not claimed either (nothing checked consistency).
    let good = Fixture::new(CONSISTENT, "good");
    let v = json(&instances(good.path(), false));
    assert_eq!(v["instances"], serde_json::json!(["http://e#b"]), "{v}");
    assert_eq!(v["completeness_guaranteed"], true, "control: {v}");
    let v = json(&instances(good.path(), true));
    assert_eq!(
        v["completeness_guaranteed"], false,
        "control, guard off: {v}"
    );
}
