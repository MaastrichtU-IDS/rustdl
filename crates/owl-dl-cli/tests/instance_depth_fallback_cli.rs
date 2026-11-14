//! #510 (2b): a single `instance` query whose bounded probe is cut by the tableau's
//! decision-depth limit retries the same `{a} ⊓ ¬C` probe once on the deep path
//! (`RUSTDL_INSTANCE_DEPTH_FALLBACK_MS`, default on), and answers what that probe decides.
//!
//! Fixtures (`HermiT` 1.4.3 adjudicated each, by the consistency of the file plus
//! `ClassAssertion(ObjectComplementOf(:Y) :a)`):
//! - [`union`]: #288's 280-union. `a : Y` is NOT entailed (`HermiT`: consistent). The
//!   bounded probe needs 280 decision levels, past `MAX_SEARCH_DEPTH` (256), so early
//!   abandon gives up after 32 hits; the retry answers "no".
//! - [`succ`]: the same 280 decisions, then `X ⊑ ∃r.W`, `W ⊑ C ⊔ D`, `C ⊑ ∀r⁻.Y`,
//!   `D ⊑ ∀r⁻.Y`. `a : Y` IS entailed (`HermiT`: inconsistent; without the `D` axiom,
//!   consistent). The deciding disjunction sits on the successor, past level 256, so the
//!   bounded probe is depth-capped; the retry answers "yes". The positive-direction canary.
//! - [`succ`] with `lazy`: plus `{b} ⊑ ≥1 f.⊤`, `f` functional (consistent). After the
//!   retry's "yes", the #369 lazy consistency check runs and reports that consistency
//!   could not be determined, so the check keys on the retry's answer, not the first
//!   probe's.
//!
//! The per-pair deadline is long in every test that needs the depth cut: the depth cut
//! is early abandon's 32nd cap hit, and on a slow (debug, loaded) run the default 750 ms
//! deadline can fire first, which is a plain deadline cut that 2b does not retry.
#![allow(clippy::unwrap_used)]

use std::fmt::Write as _;
use std::path::PathBuf;
use std::process::Command;

struct Out {
    code: i32,
    stdout: String,
    stderr: String,
}

/// The fixture file, removed on drop (also when an assertion fails).
struct Fixture(PathBuf);

impl Fixture {
    fn write(tag: &str, src: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "rustdl-instance-depth-fallback-{}-{tag}.ofn",
            std::process::id()
        ));
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

fn unions() -> String {
    let mut s = String::new();
    for k in 0..280 {
        write!(s, " ObjectUnionOf(:A{k} :B{k})").unwrap();
    }
    s
}

/// `a : X`, `X ⊑ ⊓_{k<280} (Aₖ ⊔ Bₖ)`; `a : Y` is not entailed.
fn union(tag: &str) -> Fixture {
    let src = format!(
        "Prefix(:=<http://example.org/d#>)\nOntology(<http://example.org/d>\n\
         Declaration(NamedIndividual(:a)) Declaration(Class(:Y))\n\
         SubClassOf(:X ObjectIntersectionOf({}))\nClassAssertion(:X :a)\n)\n",
        unions()
    );
    Fixture::write(tag, &src)
}

/// `a : X`, `X ⊑ ⊓_{k<280} (Aₖ ⊔ Bₖ) ⊓ ∃r.W`, `W ⊑ C ⊔ D`, `C, D ⊑ ∀r⁻.Y`; `a : Y` is
/// entailed. With `lazy`, also `{b} ⊑ ≥1 f.⊤` with `f` functional (still consistent).
fn succ(tag: &str, lazy: bool) -> Fixture {
    let inv = "ObjectAllValuesFrom(ObjectInverseOf(:r) :Y)";
    let mut src = format!(
        "Prefix(:=<http://example.org/d#>)\nPrefix(owl:=<http://www.w3.org/2002/07/owl#>)\n\
         Ontology(<http://example.org/d>\n\
         Declaration(NamedIndividual(:a)) Declaration(Class(:Y)) Declaration(ObjectProperty(:r))\n\
         SubClassOf(:X ObjectIntersectionOf({} ObjectSomeValuesFrom(:r :W)))\n\
         SubClassOf(:W ObjectUnionOf(:C :D))\nSubClassOf(:C {inv})\nSubClassOf(:D {inv})\n\
         ClassAssertion(:X :a)\n",
        unions()
    );
    if lazy {
        src.push_str(
            "Declaration(ObjectProperty(:f)) Declaration(NamedIndividual(:b))\n\
             SubClassOf(ObjectOneOf(:b) ObjectMinCardinality(1 :f owl:Thing))\n\
             FunctionalObjectProperty(:f)\n",
        );
    }
    src.push_str(")\n");
    Fixture::write(tag, &src)
}

/// Runs `rustdl` with every `RUSTDL_*` variable cleared, so only `env` sets one.
fn run(env: &[(&str, &str)], args: &[&str]) -> Out {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_rustdl"));
    for (var, _) in std::env::vars_os() {
        if var.to_string_lossy().starts_with("RUSTDL_") {
            cmd.env_remove(var);
        }
    }
    let out = cmd.envs(env.iter().copied()).args(args).output().unwrap();
    Out {
        code: out.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    }
}

const Y: &str = "http://example.org/d#Y";
const A: &str = "http://example.org/d#a";

/// Far beyond the bounded probe's run time, so its cut is the depth cut.
const LONG: (&str, &str) = ("RUSTDL_REALIZE_PAIR_TIMEOUT_MS", "60000");
const OFF: (&str, &str) = ("RUSTDL_INSTANCE_DEPTH_FALLBACK_MS", "0");

const RETRY_NOTE: &str = "a deeper retry (RUSTDL_INSTANCE_DEPTH_FALLBACK_MS";
const DEPTH_CUT: &str = "hit the tableau's decision-depth limit and reached no verdict";
const RETRY_CUT: &str = "its deeper retry was cut by its budget or the node cap";

#[track_caller]
fn assert_answer(r: &Out, code: i32, answer: &str) {
    assert_eq!(r.code, code, "{}\n{}", r.stdout, r.stderr);
    assert_eq!(r.stdout.trim(), answer, "{}\n{}", r.stdout, r.stderr);
}

/// T1: the #510 shape. A depth-capped "not entailed" is answered "no" by the retry.
#[test]
fn a_depth_cut_non_member_is_answered_no_by_the_deep_retry() {
    let f = union("no");
    let r = run(&[LONG], &["instance", f.path(), Y, A]);
    assert_answer(&r, 0, "no");
    assert!(
        r.stderr.contains(RETRY_NOTE),
        "the retry is named: {}",
        r.stderr
    );
    assert!(!r.stderr.contains(DEPTH_CUT), "{}", r.stderr);

    // Off, the same query is #514's depth cut: the retry is what answers it.
    let r = run(&[LONG, OFF], &["instance", f.path(), Y, A]);
    assert_answer(&r, 3, "unknown");
    assert!(r.stderr.contains(DEPTH_CUT), "{}", r.stderr);
    assert!(!r.stderr.contains(RETRY_NOTE), "{}", r.stderr);
}

/// T2: the positive direction. A depth-capped entailed membership is answered "yes".
#[test]
fn a_depth_cut_member_is_answered_yes_by_the_deep_retry() {
    let f = succ("yes", false);
    let r = run(&[LONG], &["instance", f.path(), Y, A]);
    assert_answer(&r, 0, "yes");
    assert!(r.stderr.contains(RETRY_NOTE), "{}", r.stderr);

    let r = run(&[LONG, OFF], &["instance", f.path(), Y, A]);
    assert_answer(&r, 3, "unknown");
    assert!(r.stderr.contains(DEPTH_CUT), "{}", r.stderr);
}

/// T5: the retry cut by its own budget is a give-up, reported as a budget cut that its
/// budget can recover, never as a "no" and never with the depth cut's "does not help".
#[test]
fn a_cut_deep_retry_is_unknown_with_budget_advice() {
    let f = union("retry-cut");
    let r = run(
        &[LONG, ("RUSTDL_INSTANCE_DEPTH_FALLBACK_MS", "1")],
        &["instance", f.path(), Y, A],
    );
    assert_answer(&r, 3, "unknown");
    assert!(r.stderr.contains(RETRY_CUT), "{}", r.stderr);
    assert!(
        r.stderr.contains("Raise RUSTDL_INSTANCE_DEPTH_FALLBACK_MS"),
        "{}",
        r.stderr
    );
    assert!(!r.stderr.contains("does not help"), "{}", r.stderr);
    assert!(!r.stderr.contains(RETRY_NOTE), "{}", r.stderr);
}

/// T4: a cut that is not a depth cut is not retried. Early abandon off and a 1 ms
/// deadline end the bounded probe on its deadline (as in `instance_depth_cut_cli.rs`).
#[test]
fn a_deadline_cut_is_not_retried() {
    let f = union("deadline");
    let r = run(
        &[
            ("RUSTDL_TABLEAU_EARLY_ABANDON", "0"),
            ("RUSTDL_REALIZE_PAIR_TIMEOUT_MS", "1"),
        ],
        &["instance", f.path(), Y, A],
    );
    assert_answer(&r, 3, "unknown");
    assert!(
        r.stderr
            .contains("instance probe was cut (deadline or node cap)"),
        "{}",
        r.stderr
    );
    assert!(!r.stderr.contains("deeper retry"), "{}", r.stderr);
}

/// The #369 lazy consistency check keys on the retry's "yes": consistency is not
/// determined on this fixture, which only a check that ran can report.
#[test]
fn the_lazy_consistency_check_follows_the_retry_s_answer() {
    let f = succ("lazy", true);
    let r = run(&[LONG], &["instance", f.path(), Y, A]);
    assert_answer(&r, 0, "yes");
    assert!(
        r.stderr.contains("consistency could not be determined"),
        "the lazy check ran after the retry's yes: {}",
        r.stderr
    );

    // Control: with the retry off the answer is a depth cut and no lazy check runs.
    let r = run(&[LONG, OFF], &["instance", f.path(), Y, A]);
    assert_answer(&r, 3, "unknown");
    assert!(
        !r.stderr.contains("consistency could not be determined"),
        "{}",
        r.stderr
    );
}

/// `instances` never retries (one retry per individual would multiply): the same depth
/// cut stays a warning there while `instance` answers.
#[test]
fn instances_does_not_retry() {
    let f = union("instances");
    let r = run(&[LONG], &["instances", f.path(), Y]);
    assert!(
        r.stderr.contains("hit the tableau's decision-depth limit"),
        "{}",
        r.stderr
    );
    assert!(!r.stderr.contains("deeper retry"), "{}", r.stderr);
}
