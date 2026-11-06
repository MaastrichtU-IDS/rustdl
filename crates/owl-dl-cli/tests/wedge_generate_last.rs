//! #465 B, `RUSTDL_WEDGE_GENERATE_LAST`: on a pool with nominals, the consistency
//! wedge fires a generating clause (`∃` / `≥n` head) only once its event worklist
//! is empty, so a node bound to a nominal merges into the nominal's node before it
//! builds successors.
//!
//! Before, a fresh node built its successors first, merged into the nominal's
//! node, and the merged node (the new one survives) rebuilt them, until the
//! deadline. `consistent` then waited out `RUSTDL_CONSISTENCY_FALLBACK_MS` and the
//! main tableau answered. The reproducers are #465's case 01976 and #473's case
//! 00939, both minimised.
//!
//! The signal that the wedge itself decided is `relies_on_trusted_consistency`
//! (#392): it is set only when the verdict is the wedge's `Sat`. The inconsistent
//! canaries need a deferred clause to fire. If a deferred clause were dropped
//! instead, the wedge would find a model too early, and `consistent` would trust it.
#![allow(clippy::unwrap_used)]

use std::path::PathBuf;
use std::process::Command;

const PREFIX: &str = "Prefix(owl:=<http://www.w3.org/2002/07/owl#>)\n\
                      Prefix(:=<http://gen.rustdl/o#>)\n\
                      Ontology(<http://gen.rustdl/o>\n";

/// #465's case 01976, minimised: consistent, with a single element `a0`.
const MIN_1976: &str = "SubClassOf(:C8 :C2)
EquivalentClasses(:C2 ObjectIntersectionOf(ObjectSomeValuesFrom(:r1 :C0) ObjectSomeValuesFrom(:r0 :C8)))
EquivalentClasses(:C2 ObjectOneOf(:a0))";

/// #473's case 00939, minimised (rustdl-features): consistent, with domain `{a2}`.
const MIN_939: &str = "SubClassOf(:C0 ObjectSomeValuesFrom(:r1 ObjectSomeValuesFrom(:r0 :C0)))
SubClassOf(ObjectComplementOf(:C0) :C1)
SubClassOf(:C0 :C1)
SubClassOf(:C1 ObjectOneOf(:a2))";

/// Inconsistent: `a2 ∈ C0` needs an `r1`-successor `y` with an `r0`-successor, so
/// `y ∈ D ⊑ ¬C1`, while every node is in `C0 ⊔ C1 ⊑ C1`. Only a fired (deferred)
/// `∃` finds it.
const INC_EXISTS: &str = "SubClassOf(:C0 ObjectSomeValuesFrom(:r1 ObjectSomeValuesFrom(:r0 :C0)))
SubClassOf(ObjectComplementOf(:C0) :C1)
SubClassOf(:C0 :C1)
SubClassOf(:C1 ObjectOneOf(:a2))
SubClassOf(ObjectSomeValuesFrom(:r0 owl:Thing) :D)
SubClassOf(:D ObjectComplementOf(:C1))
ClassAssertion(:C0 :a2)";

/// Inconsistent: the `≥2 r.E` successors are both `a2`, so they cannot be
/// distinct. Only a fired (deferred) `≥n` finds it.
const INC_AT_LEAST: &str = "SubClassOf(owl:Thing :C0)
SubClassOf(:C0 ObjectMinCardinality(2 :r :E))
SubClassOf(:E :C1)
SubClassOf(:C1 ObjectOneOf(:a2))";

/// Consistent: the control for `INC_AT_LEAST`, with two distinct individuals.
const CON_AT_LEAST: &str = "SubClassOf(owl:Thing :C0)
SubClassOf(:C0 ObjectMinCardinality(2 :r :E))
SubClassOf(:E :C1)
SubClassOf(:C1 ObjectOneOf(:a2 :a3))
DifferentIndividuals(:a2 :a3)";

/// Inconsistent: `a2`'s `r1`-successor is `a2` itself after the merge, and `F`
/// makes every `r1`-successor of `a2` `¬C1`. The successor exists only once the
/// deferred `∃` fires at the merged node.
const INC_MERGED_SUCC: &str =
    "SubClassOf(:C0 ObjectSomeValuesFrom(:r1 ObjectSomeValuesFrom(:r0 :C0)))
SubClassOf(ObjectComplementOf(:C0) :C1)
SubClassOf(:C0 :C1)
SubClassOf(:C1 ObjectOneOf(:a2))
SubClassOf(ObjectSomeValuesFrom(:r1 owl:Thing) :F)
SubClassOf(:F ObjectAllValuesFrom(:r1 :G))
SubClassOf(:G ObjectComplementOf(:C1))
ClassAssertion(:C0 :a2)";

/// Writes a fixture to a per-test file (tests run in parallel).
struct Fixture(PathBuf);

impl Fixture {
    fn new(tag: &str, axioms: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "rustdl-wedge-generate-last-{}-{tag}.ofn",
            std::process::id()
        ));
        std::fs::write(&path, format!("{PREFIX}{axioms}\n)\n")).unwrap();
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
    json: serde_json::Value,
    stderr: String,
}

/// `consistent --json` with no inherited `RUSTDL_*` settings, plus `env`.
fn consistent(fixture: &Fixture, env: &[(&str, &str)]) -> Run {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_rustdl"));
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("RUSTDL_") {
            cmd.env_remove(key);
        }
    }
    let out = cmd
        .args(["consistent", "--json", fixture.path()])
        .envs(env.iter().copied())
        .output()
        .unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    Run {
        json: serde_json::from_str(&stdout)
            .unwrap_or_else(|e| panic!("not JSON ({e}): {stdout:?}")),
        stderr: String::from_utf8(out.stderr).unwrap(),
    }
}

/// A generous wedge budget: the deferred order decides these inputs in
/// milliseconds, while the old order never settles at any budget.
const BUDGET: (&str, &str) = ("RUSTDL_CONSISTENCY_FALLBACK_MS", "20000");

fn assert_wedge_decides_consistent(tag: &str, axioms: &str) {
    let f = Fixture::new(tag, axioms);
    let r = consistent(&f, &[BUDGET]);
    assert_eq!(r.json["consistent"], true, "[{tag}] {}", r.stderr);
    assert_eq!(
        r.json["relies_on_trusted_consistency"], true,
        "[{tag}] the consistency wedge must settle and decide this itself (#465 B); \
         `false` means it was cut and the main tableau answered"
    );
}

#[test]
fn the_wedge_decides_case_01976() {
    assert_wedge_decides_consistent("min1976", MIN_1976);
}

#[test]
fn the_wedge_decides_case_00939() {
    assert_wedge_decides_consistent("min939", MIN_939);
}

/// The defect, pinned on the old order: with the flag off the wedge is cut at
/// its (short) budget, and the main tableau answers.
#[test]
fn flag_off_restores_the_old_order() {
    let f = Fixture::new("min939-off", MIN_939);
    let r = consistent(
        &f,
        &[
            ("RUSTDL_WEDGE_GENERATE_LAST", "0"),
            ("RUSTDL_CONSISTENCY_FALLBACK_MS", "500"),
        ],
    );
    assert_eq!(r.json["consistent"], true, "{}", r.stderr);
    assert_eq!(
        r.json["relies_on_trusted_consistency"], false,
        "with the old order the wedge does not settle (#465)"
    );
}

/// Each inconsistent canary is decided by the wedge (the trace names it), so it
/// exercises the deferred clause, and must stay inconsistent with the flag on
/// and off.
fn assert_wedge_proves_inconsistent(tag: &str, axioms: &str) {
    let f = Fixture::new(tag, axioms);
    for flag in ["1", "0"] {
        let r = consistent(
            &f,
            &[
                BUDGET,
                ("RUSTDL_WEDGE_GENERATE_LAST", flag),
                ("RUSTDL_TRACE", "1"),
            ],
        );
        assert_eq!(
            r.json["consistent"], false,
            "[{tag}, flag {flag}] inconsistent; a dropped deferred clause gives a \
             trusted `consistent`. stderr: {}",
            r.stderr
        );
        assert!(
            r.stderr.contains("consistency: wedge Unsat"),
            "[{tag}, flag {flag}] precondition: the wedge must decide this canary, or \
             it does not exercise the deferral. stderr: {}",
            r.stderr
        );
    }
}

#[test]
fn a_deferred_exists_still_fires() {
    assert_wedge_proves_inconsistent("inc-exists", INC_EXISTS);
}

#[test]
fn a_deferred_at_least_still_fires() {
    assert_wedge_proves_inconsistent("inc-at-least", INC_AT_LEAST);
}

#[test]
fn a_deferred_exists_fires_at_the_merged_node() {
    assert_wedge_proves_inconsistent("inc-merged-succ", INC_MERGED_SUCC);
}

/// The `≥n` control is consistent, and is a reproducer of its own: with the old
/// order the wedge never settles on it either (so that arm gets a short budget).
#[test]
fn the_at_least_control_stays_consistent() {
    assert_wedge_decides_consistent("con-at-least", CON_AT_LEAST);
    let f = Fixture::new("con-at-least-off", CON_AT_LEAST);
    let r = consistent(
        &f,
        &[
            ("RUSTDL_WEDGE_GENERATE_LAST", "0"),
            ("RUSTDL_CONSISTENCY_FALLBACK_MS", "500"),
        ],
    );
    assert_eq!(r.json["consistent"], true, "[flag off] {}", r.stderr);
}
