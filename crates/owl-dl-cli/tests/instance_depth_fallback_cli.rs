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

/// #510 (C)'s F3: each union's second disjunct heads a 50-class ∃-chain, so a small node
/// cap stops the bounded probe right after its first depth-cap hits (as in
/// `instance_depth_cut_cli.rs`). The deep retry's first descent takes the ∀-disjuncts all
/// the way down and finds a model, so it decides "no" under the same cap.
fn node_chain(tag: &str) -> Fixture {
    let mut src = String::from(
        "Prefix(:=<http://example.org/d#>)\nOntology(<http://example.org/d>\n\
         Declaration(NamedIndividual(:a)) Declaration(Class(:Y)) Declaration(Class(:A))\n\
         SubClassOf(:X ObjectIntersectionOf(",
    );
    for k in 0..280 {
        write!(
            src,
            " ObjectUnionOf(ObjectAllValuesFrom(:r{k} :A) ObjectSomeValuesFrom(:s{k} :W0))"
        )
        .unwrap();
    }
    src.push_str("))\n");
    for i in 0..50 {
        writeln!(
            src,
            "SubClassOf(:W{i} ObjectSomeValuesFrom(:p :W{}))",
            i + 1
        )
        .unwrap();
    }
    src.push_str("ClassAssertion(:X :a)\n)\n");
    Fixture::write(tag, &src)
}

/// #510 (C)'s F4: a pigeonhole forced only under `¬Y` (9 pigeons, 8 holes). Its decision
/// depth is at most 9, so it never hits the depth cap, and the default 750 ms deadline
/// cuts it: a budget cut with no hit, whatever the host's speed. Never run it
/// deadline-free: it does not finish.
fn pigeonhole(tag: &str) -> Fixture {
    const HOLES: usize = 8;
    let p = |i: usize, j: usize| format!(":P{i}_{j}");
    let mut src = String::from(
        "Prefix(owl:=<http://www.w3.org/2002/07/owl#>)\nPrefix(:=<http://example.org/d#>)\n\
         Ontology(<http://example.org/d>\n\
         Declaration(NamedIndividual(:a)) Declaration(Class(:Y)) Declaration(Class(:X))\n",
    );
    let holes = |i: usize| (0..HOLES).map(|j| p(i, j)).collect::<Vec<_>>().join(" ");
    for i in 0..HOLES {
        writeln!(src, "SubClassOf(:X ObjectUnionOf({}))", holes(i)).unwrap();
    }
    writeln!(
        src,
        "SubClassOf(owl:Thing ObjectUnionOf(:Y {}))",
        holes(HOLES)
    )
    .unwrap();
    for j in 0..HOLES {
        for i in 0..=HOLES {
            for k in i + 1..=HOLES {
                writeln!(src, "DisjointClasses({} {})", p(i, j), p(k, j)).unwrap();
            }
        }
    }
    src.push_str("ClassAssertion(:X :a)\n)\n");
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

/// T4: a budget cut with no depth-cap hit is not retried. The pigeonhole (F4) never
/// reaches the depth cap, and the default deadline cuts it. (The 280-union at 1 ms with
/// early abandon off is no control since #510 (C): a hit inside 1 ms makes it a
/// `depth_cap_hit`, which is retried.)
#[test]
fn a_deadline_cut_is_not_retried() {
    let f = pigeonhole("deadline");
    let r = run(&[], &["instance", f.path(), Y, A]);
    assert_answer(&r, 3, "unknown");
    assert!(
        r.stderr
            .contains("instance probe was cut (deadline or node cap)"),
        "{}",
        r.stderr
    );
    assert!(!r.stderr.contains("deeper retry"), "{}", r.stderr);
    assert!(!r.stderr.contains("decision-depth"), "no hit: {}", r.stderr);
}

/// T5, #510 (C) wiring: a budget cut after a depth-cap hit (F1: early abandon armed but
/// never latching; F2: off) is retried, and the retry answers "no" with its note. The
/// first hit comes within ~0.4 CPU-s in debug, far inside the 10 s deadline.
#[test]
fn a_budget_cut_after_a_depth_cap_hit_is_answered_by_the_deep_retry() {
    let f = union("hit");
    for (case, ea) in [
        ("F1", ("RUSTDL_TABLEAU_EARLY_ABANDON_HITS", "1000000")),
        ("F2", ("RUSTDL_TABLEAU_EARLY_ABANDON", "0")),
    ] {
        let deadline = ("RUSTDL_REALIZE_PAIR_TIMEOUT_MS", "10000");
        let r = run(&[ea, deadline], &["instance", f.path(), Y, A]);
        assert_answer(&r, 0, "no");
        assert!(r.stderr.contains(RETRY_NOTE), "[{case}] {}", r.stderr);
        assert!(!r.stderr.contains("warning"), "[{case}] {}", r.stderr);

        // Control: off, it is #510 (C)'s hit-then-budget cut.
        let r = run(&[ea, deadline, OFF], &["instance", f.path(), Y, A]);
        assert_answer(&r, 3, "unknown");
        assert!(
            r.stderr.contains("may help as well as a larger budget"),
            "[{case}] {}",
            r.stderr
        );
    }
}

/// T6, #510 (C) wiring: F3's node-cap stop after a hit is retried under the same node
/// cap, and the retry decides "no" (its first descent finds a model without the chain).
#[test]
fn a_node_cap_cut_after_a_depth_cap_hit_is_retried() {
    let f = node_chain("hit-nodes");
    let env = [
        ("RUSTDL_TABLEAU_EARLY_ABANDON_HITS", "1000000"),
        LONG,
        ("RUSTDL_QUERY_MAX_NODES", "20"),
    ];
    let r = run(&env, &["instance", f.path(), Y, A]);
    assert_answer(&r, 0, "no");
    assert!(r.stderr.contains(RETRY_NOTE), "{}", r.stderr);
}

/// T7, #510 (C) wiring: the hit-triggered retry can answer "yes" too, and the #369 lazy
/// check follows it (early abandon off; the first hit comes well inside 10 s).
#[test]
fn a_hit_triggered_retry_can_answer_yes_and_the_lazy_check_follows() {
    let f = succ("hit-lazy", true);
    let env = [
        ("RUSTDL_TABLEAU_EARLY_ABANDON", "0"),
        ("RUSTDL_REALIZE_PAIR_TIMEOUT_MS", "10000"),
    ];
    let r = run(&env, &["instance", f.path(), Y, A]);
    assert_answer(&r, 0, "yes");
    assert!(r.stderr.contains(RETRY_NOTE), "{}", r.stderr);
    assert!(
        r.stderr.contains("consistency could not be determined"),
        "the lazy check ran after the retry's yes: {}",
        r.stderr
    );
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

/// Plan R1: `justify` / `repair` ask one instance check per `QuickXplain` step, so they
/// use the entry without the retry. Pinned on observables of this PR alone: the retry's
/// `RUSTDL_TRACE` line never shows, and the check stays a give-up (a retry would decide
/// "not entailed", as the deadline-free probe does). The trace line is proved to fire on
/// `instance` first.
#[test]
fn justify_and_repair_never_retry() {
    const TRACE: (&str, &str) = ("RUSTDL_TRACE", "1");
    const RETRY_TRACE: &str = "bounded probe depth-capped; deep retry";
    let f = union("justify");
    let r = run(&[LONG, TRACE], &["instance", f.path(), Y, A]);
    assert_answer(&r, 0, "no");
    assert!(
        r.stderr.contains(RETRY_TRACE),
        "the instrument fires: {}",
        r.stderr
    );

    for cmd in ["justify", "repair"] {
        let r = run(&[LONG, TRACE], &[cmd, f.path(), "instance", A, Y]);
        assert_eq!(r.code, 3, "{cmd}: a give-up: {}\n{}", r.stdout, r.stderr);
        assert!(r.stdout.starts_with("unknown"), "{cmd}: {}", r.stdout);
        assert!(
            !r.stderr.contains(RETRY_TRACE),
            "{cmd} retried: {}",
            r.stderr
        );

        // Control: the deep probe does decide this check, so a retry would have shown.
        let r = run(
            &[("RUSTDL_REALIZE_PAIR_TIMEOUT_MS", "0")],
            &[cmd, f.path(), "instance", A, Y],
        );
        assert_eq!(r.code, 0, "{cmd}: {}\n{}", r.stdout, r.stderr);
        assert!(r.stdout.starts_with("not entailed"), "{cmd}: {}", r.stdout);
    }
}
