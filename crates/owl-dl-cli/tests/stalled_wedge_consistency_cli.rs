//! #571: when classify's consistency wedge returns `Stalled`, the pre-check runs the
//! main tableau in what is left of the wedge's deadline. A model decides the KB
//! consistent (`consistency_undetermined: false`); a give-up of the main tableau
//! keeps `consistency_undetermined: true`.
//!
//! The fixture is the #564 r-cycle with `A(d)`: the merge-churn guard stops the
//! consistency wedge, and `HermiT` says the KB is consistent (with `Q ≡ ⊥`).
#![allow(clippy::unwrap_used)]

use std::path::PathBuf;
use std::process::Command;

const R_CYCLE_A: &str = r"Prefix(:=<http://x.org/f#>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Ontology(<http://x.org/f>
Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:Q))
Declaration(ObjectProperty(:r)) Declaration(ObjectProperty(:s)) Declaration(ObjectProperty(:t))
Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:d))
SubClassOf(:Q ObjectMinCardinality(2 :r :A))
SubClassOf(:A ObjectSomeValuesFrom(:t :B))
SubClassOf(:B ObjectSomeValuesFrom(ObjectInverseOf(:s) ObjectOneOf(:a)))
SubClassOf(:B ObjectSomeValuesFrom(:r :A))
FunctionalObjectProperty(:s)
InverseFunctionalObjectProperty(:t)
ClassAssertion(:A :d)
)
";

/// Writes the fixture to a per-test file (tests run in parallel).
struct Fixture(PathBuf);

impl Fixture {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "rustdl-stalled-wedge-{}-{tag}.ofn",
            std::process::id()
        ));
        std::fs::write(&path, R_CYCLE_A).unwrap();
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

/// `classify --json` with no inherited `RUSTDL_*` settings, plus `env`.
fn classify_json(path: &str, env: &[(&str, &str)]) -> serde_json::Value {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_rustdl"));
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("RUSTDL_") {
            cmd.env_remove(key);
        }
    }
    let out = cmd
        .args(["classify", "--json", path])
        .envs(env.iter().copied())
        .output()
        .unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    serde_json::from_str(&stdout).unwrap_or_else(|e| {
        panic!(
            "{env:?}: {e}: {stdout} / {}",
            String::from_utf8_lossy(&out.stderr)
        )
    })
}

/// [`classify_json`]'s `(consistent, consistency_undetermined)`.
fn classify(path: &str, env: &[(&str, &str)]) -> (bool, bool) {
    let v = classify_json(path, env);
    (
        v["consistent"].as_bool().unwrap(),
        v["consistency_undetermined"].as_bool().unwrap(),
    )
}

/// The diagnostics that count fallback firings: the wedge stalled on the
/// merge-churn guard, and the fallback's verdict, `null` when it did not run.
#[test]
fn the_json_reports_the_stall_and_the_fallback_verdict() {
    let f = Fixture::new("json");
    let fields = |env: &[(&str, &str)]| {
        let v = classify_json(f.path(), env);
        (
            v["consistency_wedge_stalled"].as_bool().unwrap(),
            v["consistency_wedge_merge_churn_stops"].as_u64().unwrap() > 0,
            v["stalled_wedge_fallback"].clone(),
        )
    };
    assert_eq!(
        fields(&[]),
        (true, true, serde_json::json!("consistent")),
        "default"
    );
    assert_eq!(
        fields(&[
            ("RUSTDL_MAX_NODES", "1"),
            ("RUSTDL_CLASSIFY_CONSISTENCY_PROBE", "0")
        ]),
        (true, true, serde_json::json!("gave_up")),
        "node cap"
    );
    assert_eq!(
        fields(&[("RUSTDL_CLASSIFY_STALLED_WEDGE_FALLBACK", "0")]),
        (true, true, serde_json::Value::Null),
        "fallback off"
    );
}

#[test]
fn a_stalled_wedge_is_decided_by_the_main_tableau() {
    let f = Fixture::new("decided");
    assert_eq!(classify(f.path(), &[]), (true, false), "default: decided");
    assert_eq!(
        classify(f.path(), &[("RUSTDL_CLASSIFY_STALLED_WEDGE_FALLBACK", "0")]),
        (true, true),
        "fallback off: undetermined"
    );
}

/// The give-up arm, made deterministic with a node cap rather than a tiny deadline
/// (which would race the wedge's own entry check): the main tableau stops at one
/// node and answers no verdict, so the pre-check stays undetermined, never a
/// guessed `consistent` or `inconsistent`. Classify's later consistency probe
/// (#446) is switched off: under the same cap it gives up too and would set
/// `consistency_undetermined` on its own, hiding a fallback that read its give-up
/// as a model.
#[test]
fn a_main_tableau_give_up_keeps_the_kb_undetermined() {
    let f = Fixture::new("give-up");
    assert_eq!(
        classify(
            f.path(),
            &[
                ("RUSTDL_MAX_NODES", "1"),
                ("RUSTDL_CLASSIFY_CONSISTENCY_PROBE", "0"),
            ]
        ),
        (true, true),
        "node cap: undetermined"
    );
}

/// The no-deadline cap. With `RUSTDL_CLASSIFY_INCONSISTENCY_MS=0` the pre-check
/// routes have no budget of their own (and the CLI sets no global deadline), so the
/// fallback is capped at `RUSTDL_CONSISTENCY_FALLBACK_MS` instead of running
/// unbounded. A zero cap leaves it no time, so the KB stays undetermined; a fallback
/// that ran with no deadline would decide it. The #446 probe is off as above.
#[test]
fn with_no_route_budget_the_fallback_is_capped() {
    let f = Fixture::new("cap");
    let no_budget = ("RUSTDL_CLASSIFY_INCONSISTENCY_MS", "0");
    let no_probe = ("RUSTDL_CLASSIFY_CONSISTENCY_PROBE", "0");
    assert_eq!(
        classify(f.path(), &[no_budget, no_probe]),
        (true, false),
        "no route budget, default cap: decided"
    );
    assert_eq!(
        classify(
            f.path(),
            &[no_budget, no_probe, ("RUSTDL_CONSISTENCY_FALLBACK_MS", "0")]
        ),
        (true, true),
        "no route budget, zero cap: undetermined"
    );
}
