//! #510: an `instance` probe cut by the tableau's decision-depth limit is reported as
//! such, and the advice says what recovers it.
//!
//! The fixture is #288's 280-union: `a : X` with `X ⊑ ⊓ₖ (Aₖ ⊔ Bₖ)`, `k < 280`, and an
//! unrelated `Y`. `a : Y` is not entailed, but the bounded probe (`{a} ⊓ ¬Y`) needs 280
//! decision levels, past `MAX_SEARCH_DEPTH` (256). Every branch hits the cap and early
//! abandon gives up after 32 hits, long before any deadline: the census's ORE shape (#510:
//! exactly 32 cap hits per trace). A larger timeout or node cap does not change that,
//! and the old message advised both. The deadline-free probe
//! (`RUSTDL_REALIZE_PAIR_TIMEOUT_MS=0`) searches deeper and answers.
//!
//! The follow-up covers `instances-expr`, `realize`, `justify` and `repair` on a second
//! fixture, [`Fixture::forall`]: the same 280 decisions with ∀-disjuncts over one class,
//! so `realize` doesn't pay a depth cut for each of 560 named classes.
#![allow(clippy::unwrap_used)]

use std::fmt::Write as _;
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

struct Out {
    code: i32,
    stdout: String,
    stderr: String,
}

/// The fixture file, removed on drop (also when an assertion fails).
struct Fixture(PathBuf);

impl Fixture {
    fn new(tag: &str) -> Self {
        let mut src = String::from(
            "Prefix(:=<http://example.org/d#>)\nOntology(<http://example.org/d>\n\
             Declaration(NamedIndividual(:a)) Declaration(Class(:Y))\n\
             SubClassOf(:X ObjectIntersectionOf(",
        );
        for k in 0..280 {
            write!(src, " ObjectUnionOf(:A{k} :B{k})").unwrap();
        }
        src.push_str("))\nClassAssertion(:X :a)\n)\n");
        Self::write(tag, &src)
    }

    /// `X ⊑ ⊓_{k<280} (∀r_k.A ⊔ ∀s_k.A)`, `a : X`: named classes {X, Y, A} only. The
    /// ∀-disjuncts make no successors, so each union is one decision and `{a} ⊓ ¬Y`
    /// needs 280 levels; early abandon gives up after 32 cap hits (measured:
    /// `depth0=32 abandoned=1` at both 750 ms and 60 s). With `a_is_y`, `a : Y` is also
    /// asserted: the query is then entailed, and repair's check that removing it breaks
    /// the entailment is the depth-cut probe (a give-up inside the search).
    fn forall(tag: &str, a_is_y: bool) -> Self {
        let mut src = String::from(
            "Prefix(:=<http://example.org/d#>)\nOntology(<http://example.org/d>\n\
             Declaration(NamedIndividual(:a)) Declaration(Class(:Y)) Declaration(Class(:A))\n\
             SubClassOf(:X ObjectIntersectionOf(",
        );
        for k in 0..280 {
            write!(
                src,
                " ObjectUnionOf(ObjectAllValuesFrom(:r{k} :A) ObjectAllValuesFrom(:s{k} :A))"
            )
            .unwrap();
        }
        src.push_str("))\nClassAssertion(:X :a)\n");
        if a_is_y {
            src.push_str("ClassAssertion(:Y :a)\n");
        }
        src.push_str(")\n");
        Self::write(tag, &src)
    }

    fn write(tag: &str, src: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "rustdl-instance-depth-cut-{}-{tag}.ofn",
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
const X: &str = "http://example.org/d#X";

/// A per-pair deadline far beyond the probe's run time, so the cut cannot be the
/// deadline even on a loaded host, and the test pins that a longer timeout does not help.
const LONG_DEADLINE_MS: &str = "60000";

#[test]
fn a_depth_cut_instance_probe_names_the_limit_and_the_advice_that_helps() {
    let f = Fixture::new("depth");
    let p = f.path();
    let env = [("RUSTDL_REALIZE_PAIR_TIMEOUT_MS", LONG_DEADLINE_MS)];
    let t = Instant::now();
    let r = run(&env, &["instance", p, Y, A]);
    // Early abandon ends the probe quickly; a run near the 60 s deadline would be a
    // deadline cut, not the depth cut this test is about.
    assert!(
        t.elapsed() < Duration::from_secs(30),
        "the probe must end on the depth limit, well before its deadline: {:?}",
        t.elapsed()
    );
    assert_eq!(r.code, 3, "a cut is a give-up: {}\n{}", r.stdout, r.stderr);
    assert_eq!(r.stdout.trim(), "unknown", "{}", r.stdout);
    assert!(
        r.stderr.contains("hit the tableau's decision-depth limit"),
        "the cut is named: {}",
        r.stderr
    );
    assert!(
        r.stderr.contains("does not help") && r.stderr.contains("RUSTDL_REALIZE_PAIR_TIMEOUT_MS=0"),
        "the advice says a larger budget does not help and what does: {}",
        r.stderr
    );
    assert!(
        !r.stderr.contains("Raise RUSTDL_REALIZE_PAIR_TIMEOUT_MS"),
        "no advice to raise a budget that cannot recover a depth cut: {}",
        r.stderr
    );

    // The advice is right: the deadline-free probe answers the same query.
    let r = run(
        &[("RUSTDL_REALIZE_PAIR_TIMEOUT_MS", "0")],
        &["instance", p, Y, A],
    );
    assert_eq!(r.code, 0, "{}\n{}", r.stdout, r.stderr);
    assert_eq!(r.stdout.trim(), "no", "{}", r.stdout);

    // And `instances` (the same flags, OR-ed over the individuals) says so too.
    let r = run(&env, &["instances", p, Y]);
    assert!(
        r.stderr.contains("hit the tableau's decision-depth limit"),
        "{}",
        r.stderr
    );
}

#[test]
fn a_deadline_cut_instance_probe_is_not_reported_as_a_depth_cut() {
    // Early abandon off and a 1 ms per-pair deadline: the plain 280-union never exhausts
    // (every branch hits the cap and backtracks), so the probe always ends on the
    // deadline. The cut is NOT depth-capped and is reported as a deadline or node-cap
    // cut. The budget advice that comes with it cannot recover this fixture, which keeps
    // hitting the depth cap whatever the deadline: with early abandon off, depth-cap hits
    // before a deadline are not tracked (docs/known-limitations/bounded-probe-depth-cap.md).
    let f = Fixture::new("deadline");
    let p = f.path();
    let env = [
        ("RUSTDL_TABLEAU_EARLY_ABANDON", "0"),
        ("RUSTDL_REALIZE_PAIR_TIMEOUT_MS", "1"),
    ];
    let r = run(&env, &["instance", p, Y, A]);
    assert_eq!(r.code, 3, "{}\n{}", r.stdout, r.stderr);
    assert!(
        r.stderr
            .contains("instance probe was cut (deadline or node cap)"),
        "{}",
        r.stderr
    );
    assert!(
        r.stderr.contains("Raise RUSTDL_REALIZE_PAIR_TIMEOUT_MS"),
        "{}",
        r.stderr
    );
    assert!(!r.stderr.contains("decision-depth limit"), "{}", r.stderr);

    // `instances` gives the same (`Some(false)`) wording.
    let r = run(&env, &["instances", p, Y]);
    assert!(
        r.stderr
            .contains("at least one instance probe was cut (deadline or node cap)"),
        "{}",
        r.stderr
    );
    assert!(!r.stderr.contains("decision-depth limit"), "{}", r.stderr);
}

/// The depth wording of the list surfaces (`warn_probes_cut`).
const LIST_DEPTH: &str = "at least one instance probe hit the tableau's decision-depth limit";

#[test]
fn instances_expr_and_realize_name_a_depth_cut_and_the_advice_that_helps() {
    let f = Fixture::forall("list-depth", false);
    let p = f.path();
    let env = [("RUSTDL_REALIZE_PAIR_TIMEOUT_MS", LONG_DEADLINE_MS)];
    for (args, what) in [
        (vec!["instances-expr", p, ":Y"], "instances-expr"),
        (vec!["realize", p], "realize"),
    ] {
        let t = Instant::now();
        let r = run(&env, &args);
        assert!(
            t.elapsed() < Duration::from_secs(60),
            "{what}: {:?}",
            t.elapsed()
        );
        assert_eq!(
            r.code, 0,
            "{what}: a list answer: {}\n{}",
            r.stdout, r.stderr
        );
        assert!(
            r.stderr.contains(&format!("warning: {what}: {LIST_DEPTH}")),
            "{}",
            r.stderr
        );
        assert!(
            r.stderr.contains("does not help")
                && r.stderr.contains("RUSTDL_REALIZE_PAIR_TIMEOUT_MS=0"),
            "{what}: {}",
            r.stderr
        );
        assert!(
            !r.stderr.contains("(deadline or node cap)"),
            "{what}: {}",
            r.stderr
        );
    }
    // `realize` also says what `=0` costs there, and its sound answer stands.
    let r = run(&env, &["realize", p]);
    assert!(r.stdout.contains(&format!("{A}\t{X}")), "{}", r.stdout);
    assert!(r.stderr.contains("classification step"), "{}", r.stderr);

    // The advice is right: the deadline-free probe answers with no cut.
    let free = [("RUSTDL_REALIZE_PAIR_TIMEOUT_MS", "0")];
    let r = run(&free, &["instances-expr", p, ":Y"]);
    assert_eq!((r.code, r.stdout.trim()), (0, ""), "{}", r.stderr);
    assert!(!r.stderr.contains("warning"), "{}", r.stderr);
    let r = run(&free, &["realize", p]);
    assert!(r.stdout.contains(&format!("{A}\t{X}")), "{}", r.stdout);
    assert!(!r.stderr.contains("warning"), "{}", r.stderr);
}

#[test]
fn a_deadline_cut_on_instances_expr_and_realize_is_not_a_depth_cut() {
    // As the `instance` control: early abandon off and a 1 ms deadline, so every cut
    // is a deadline cut.
    let f = Fixture::forall("list-deadline", false);
    let p = f.path();
    let env = [
        ("RUSTDL_TABLEAU_EARLY_ABANDON", "0"),
        ("RUSTDL_REALIZE_PAIR_TIMEOUT_MS", "1"),
    ];
    for (args, what) in [
        (vec!["instances-expr", p, ":Y"], "instances-expr"),
        (vec!["realize", p], "realize"),
    ] {
        let r = run(&env, &args);
        assert!(
            r.stderr.contains(&format!(
                "warning: {what}: at least one instance probe was cut (deadline or node cap)"
            )),
            "{}",
            r.stderr
        );
        assert!(
            !r.stderr.contains("decision-depth limit"),
            "{what}: {}",
            r.stderr
        );
    }
}

/// The depth wording of `justify` / `repair`'s top-level check.
const TOP_DEPTH: &str =
    "a reasoner check (an instance check) hit the tableau's decision-depth limit";

#[test]
fn justify_and_repair_name_a_depth_cut_of_their_instance_check() {
    let f = Fixture::forall("explain-depth", false);
    let p = f.path();
    let env = [("RUSTDL_REALIZE_PAIR_TIMEOUT_MS", LONG_DEADLINE_MS)];
    for cmd in ["justify", "repair"] {
        let r = run(&env, &[cmd, p, "instance", A, Y]);
        assert_eq!(r.code, 3, "{cmd}: a give-up: {}\n{}", r.stdout, r.stderr);
        assert!(r.stdout.starts_with("unknown"), "{cmd}: {}", r.stdout);
        assert!(r.stderr.contains(TOP_DEPTH), "{cmd}: {}", r.stderr);
        assert!(
            r.stderr.contains("RUSTDL_REALIZE_PAIR_TIMEOUT_MS=0"),
            "{cmd}: {}",
            r.stderr
        );
        assert!(
            !r.stderr.contains("node cap, RUSTDL_MAX_NODES"),
            "{cmd}: {}",
            r.stderr
        );

        // The advice is right: the deadline-free probe decides "not entailed".
        let r = run(
            &[("RUSTDL_REALIZE_PAIR_TIMEOUT_MS", "0")],
            &[cmd, p, "instance", A, Y],
        );
        assert_eq!(r.code, 0, "{cmd}: {}\n{}", r.stdout, r.stderr);
        assert!(r.stdout.starts_with("not entailed"), "{cmd}: {}", r.stdout);
    }

    // A give-up inside the search: `a : Y` is asserted, and repair's check that
    // removing it breaks the entailment is the depth-cut probe.
    let f = Fixture::forall("explain-search-depth", true);
    let p = f.path();
    let r = run(&env, &["repair", p, "instance", A, Y]);
    assert!(
        r.stderr.contains(
            "a reasoner check gave up during the search; at least one instance check hit the \
             tableau's decision-depth limit"
        ),
        "{}",
        r.stderr
    );
    let r = run(
        &[("RUSTDL_REALIZE_PAIR_TIMEOUT_MS", "0")],
        &["repair", p, "instance", A, Y],
    );
    assert!(
        r.stdout.contains("repair 1 (remove 1 axiom(s))"),
        "{}",
        r.stdout
    );
    assert!(!r.stderr.contains("warning"), "{}", r.stderr);
}

#[test]
fn a_deadline_cut_in_justify_and_repair_is_not_a_depth_cut() {
    let env = [
        ("RUSTDL_TABLEAU_EARLY_ABANDON", "0"),
        ("RUSTDL_REALIZE_PAIR_TIMEOUT_MS", "1"),
    ];
    let f = Fixture::forall("explain-deadline", false);
    let r = run(&env, &["justify", f.path(), "instance", A, Y]);
    assert_eq!(r.code, 3, "{}\n{}", r.stdout, r.stderr);
    assert!(
        r.stderr.contains("gave up (node cap, RUSTDL_MAX_NODES)"),
        "{}",
        r.stderr
    );
    assert!(
        r.stderr
            .contains("its RUSTDL_REALIZE_PAIR_TIMEOUT_MS deadline"),
        "{}",
        r.stderr
    );
    assert!(!r.stderr.contains("decision-depth limit"), "{}", r.stderr);

    let f = Fixture::forall("explain-search-deadline", true);
    let r = run(&env, &["repair", f.path(), "instance", A, Y]);
    assert!(
        r.stderr
            .contains("a reasoner check gave up during the search (node cap, RUSTDL_MAX_NODES)"),
        "{}",
        r.stderr
    );
    assert!(!r.stderr.contains("decision-depth limit"), "{}", r.stderr);
}
