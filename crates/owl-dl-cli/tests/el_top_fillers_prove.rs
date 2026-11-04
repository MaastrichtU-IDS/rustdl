//! #435 / #436: `prove` on the ⊤-filler shapes. The new synthetic classes (the
//! ⊤-witness `TW_r`, read `∃r⁻.⊤`, and the trigger ⊤-marker `T_r ≡ ∃r.⊤`) must
//! render (the JSON renderer refuses an unregistered synthetic id), the step proof
//! must verify, and it must carry no gap: `TW_r ⊑ F` cites `Range(r, F)`, and the
//! marker step is a definition.
#![allow(clippy::unwrap_used)]

use std::path::PathBuf;
use std::process::Command;

struct Fixture(PathBuf);

impl Fixture {
    fn new(tag: &str, body: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "rustdl-top-filler-prove-{}-{tag}.ofn",
            std::process::id()
        ));
        let src = format!(
            "Prefix(:=<http://ex/#>)\nPrefix(owl:=<http://www.w3.org/2002/07/owl#>)\n\
             Ontology(<http://ex/o>\n\
             Declaration(Class(:X)) Declaration(Class(:F)) Declaration(Class(:W)) \
             Declaration(Class(:B)) Declaration(Class(:T)) Declaration(ObjectProperty(:r)) \
             Declaration(ObjectProperty(:s))\n{body}\n)\n"
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

fn run(args: &[&str]) -> (i32, String, String) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_rustdl"));
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("RUSTDL_") {
            cmd.env_remove(key);
        }
    }
    let out = cmd.args(args).output().unwrap();
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8(out.stdout).unwrap(),
        String::from_utf8(out.stderr).unwrap(),
    )
}

/// The proof of `X ⊑ W` verifies, has no gap, and renders as JSON.
fn proves_cleanly(tag: &str, body: &str) {
    let f = Fixture::new(tag, body);
    let (x, w) = ("http://ex/#X", "http://ex/#W");
    let (code, stdout, stderr) = run(&["prove", "--verify-proof", f.path(), x, w]);
    assert_eq!(code, 0, "{stderr}");
    assert!(stdout.starts_with("# step proof"), "{tag}: {stdout}");
    assert!(stderr.contains("proof verified OK"), "{tag}: {stderr}");
    assert!(!stdout.contains("proof incomplete"), "{tag}: {stdout}");
    let (code, json, stderr) = run(&["prove", "--json", f.path(), x, w]);
    assert_eq!(code, 0, "{stderr}");
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["status"], "entailed", "{tag}");
    assert_eq!(v["has_proof"], true, "{tag}");
    assert_eq!(v["proof_complete"], true, "{tag}: {json}");
}

/// #435: the witness's `Range(r)` step.
#[test]
fn top_witness_range_proof() {
    proves_cleanly(
        "witness",
        "SubClassOf(:X ObjectSomeValuesFrom(:r owl:Thing))
         ObjectPropertyRange(:r :F)
         SubClassOf(ObjectSomeValuesFrom(:r :F) :W)",
    );
}

/// #436: the trigger ⊤-marker step.
#[test]
fn nested_top_trigger_proof() {
    proves_cleanly(
        "marker",
        "SubClassOf(:X ObjectSomeValuesFrom(:r ObjectSomeValuesFrom(:s :B)))
         SubClassOf(ObjectSomeValuesFrom(:r ObjectSomeValuesFrom(:s owl:Thing)) :W)",
    );
}

/// #436, fact position: the nested witness carries `Range(s)` through a
/// conjunction synthetic (`TW_s ⊓ F`), which must render and check.
#[test]
fn nested_top_fact_proof() {
    proves_cleanly(
        "fact",
        "SubClassOf(:X ObjectSomeValuesFrom(:r ObjectSomeValuesFrom(:s owl:Thing)))
         ObjectPropertyRange(:s :F)
         SubClassOf(ObjectSomeValuesFrom(:r ObjectSomeValuesFrom(:s :F)) :W)",
    );
}

/// #436: the ⊤-marker reached by the domain rule on a chain-COMPOSED fact (the
/// `DomainFact` recording site): still a definition, not a gap.
#[test]
fn top_marker_on_a_composed_fact_proof() {
    proves_cleanly(
        "chain",
        "Declaration(ObjectProperty(:t)) Declaration(Class(:A))
         SubObjectPropertyOf(ObjectPropertyChain(:r :s) :t)
         SubClassOf(:X ObjectSomeValuesFrom(:r ObjectSomeValuesFrom(:s :B)))
         SubClassOf(:X :A)
         SubClassOf(ObjectIntersectionOf(:A ObjectSomeValuesFrom(:t owl:Thing)) :W)",
    );
}

/// #445: a step through the ⊤ key `K⊤` (`⊤ ⊑ ∃r.B` lowers as `K⊤ ⊑ ∃r.B`). `X ⊑ K⊤`
/// is a definition (`X ⊑ ⊤`) and `K⊤`'s fact cites the ⊤-LHS axiom: no gap.
#[test]
fn top_lhs_existential_proof() {
    proves_cleanly(
        "topkey",
        "SubClassOf(owl:Thing ObjectSomeValuesFrom(:r :B))
         SubClassOf(ObjectSomeValuesFrom(:r :B) :W)",
    );
}

/// #444: the marker target `M ≡ ∃s.B` gets `T` from `⊤ ⊑ T`; that step cites the
/// `⊤ ⊑ T` axiom, so the proof of `X ⊑ W` has no gap.
#[test]
fn top_subsumer_on_a_marker_target_proof() {
    proves_cleanly(
        "markertop",
        "SubClassOf(owl:Thing :T)
         SubClassOf(:X ObjectSomeValuesFrom(:r ObjectSomeValuesFrom(:s :B)))
         SubClassOf(ObjectSomeValuesFrom(:r :T) :W)",
    );
}
