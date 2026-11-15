//! #526 on the CLI: `ObjectPropertyAssertion(ObjectInverseOf(:r) :a :b)` is the edge
//! `r(b, a)`, so `Domain(r) = D` types `b` and `Range(r) = R` types `a`. `instance`
//! said `yes` to `a : D` and `b : R`, on both the full and the saturation-only path.
//! Negatives first; `HermiT` and Konclude agree on every answer here.
#![allow(clippy::unwrap_used)]

use std::path::PathBuf;
use std::process::Command;

const SRC: &str = "Prefix(:=<http://ex/#>)
Ontology(<http://ex/o>
Declaration(ObjectProperty(:r)) Declaration(Class(:D)) Declaration(Class(:R))
Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))
ObjectPropertyDomain(:r :D) ObjectPropertyRange(:r :R)
ObjectPropertyAssertion(ObjectInverseOf(:r) :a :b)
)
";

/// Writes the fixture to a per-test file (tests run in parallel).
struct Fixture(PathBuf);

impl Fixture {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "rustdl-inverse-told-{}-{tag}.ofn",
            std::process::id()
        ));
        std::fs::write(&path, SRC).unwrap();
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

/// `rustdl instance [--saturation-only] FILE CLASS IND`: the first stdout line.
fn instance(f: &Fixture, saturation_only: bool, class: &str, ind: &str) -> String {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_rustdl"));
    cmd.arg("instance");
    if saturation_only {
        cmd.arg("--saturation-only");
    }
    let out = cmd
        .args([
            f.path(),
            &format!("http://ex/#{class}"),
            &format!("http://ex/#{ind}"),
        ])
        .output()
        .unwrap();
    String::from_utf8(out.stdout)
        .unwrap()
        .lines()
        .next()
        .unwrap_or("")
        .to_string()
}

#[test]
fn instance_types_the_ends_of_the_named_edge() {
    let f = Fixture::new("instance");
    for saturation_only in [false, true] {
        for (class, ind, want) in [
            ("D", "a", "no"),
            ("R", "b", "no"),
            ("D", "b", "yes"),
            ("R", "a", "yes"),
        ] {
            assert_eq!(
                instance(&f, saturation_only, class, ind),
                want,
                "{ind} : {class} (saturation_only = {saturation_only})"
            );
        }
    }
}
