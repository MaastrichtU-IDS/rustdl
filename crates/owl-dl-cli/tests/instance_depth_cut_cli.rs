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

    /// #510 (C), F3: like [`Fixture::forall`] but each union's second disjunct is
    /// `∃s_k.W0`, the head of a 50-class ∃-chain. The first descent takes the
    /// ∀-disjuncts and hits the depth cap; backtracking into an ∃-disjunct then builds
    /// the chain, so a small node cap stops the probe right after its first hits
    /// (measured: `depth0=2`, then the node cap, 0.2 s release / 0.8 s debug).
    fn node_chain(tag: &str) -> Self {
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
        Self::write(tag, &src)
    }

    /// #510 (C), F4: a pigeonhole forced only under `¬Y`: 9 pigeons, 8 holes. `a : Y`
    /// keeps the KB consistent, and the probe `{a} ⊓ ¬Y` must refute all 8⁹ placements.
    /// Its decision depth is at most 9 by construction, so it never hits the depth cap
    /// (measured `depth0=0`) and the default 750 ms deadline cuts it: a budget cut with
    /// no depth-limit hit, whatever the host's speed. Never run it deadline-free: it
    /// does not finish.
    fn pigeonhole(tag: &str) -> Self {
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
    // #510 (C): a depth cut is not also reported as a hit before a budget cut.
    assert!(!r.stderr.contains("may help as well"), "{}", r.stderr);

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
    // The pigeonhole (F4) at the default deadline: an exponential search whose decision
    // depth stays far below the cap, so the deadline cuts it with no depth-limit hit at
    // all, on any host. It is reported as a plain deadline or node-cap cut, with neither
    // #510's depth wording nor #510 (C)'s hit clause. (Before #510 C this used the
    // 280-union at 1 ms with early abandon off, which now reports the hits it makes.)
    let f = Fixture::pigeonhole("deadline");
    let p = f.path();
    let env: [(&str, &str); 0] = [];
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

/// A labelled case: its name, the fixture path and the `RUSTDL_*` settings.
type Case<'a> = (&'a str, &'a str, Vec<(&'a str, &'a str)>);

/// #510 (C): a deadline or the node cap cut the probe after it had hit the depth cap
/// (F1: early abandon armed but never latching; F2: early abandon off; F3: a node-cap
/// stop). The budget wording stays, and the hit clause adds that `=0` may help too.
/// The first hit comes within ~0.4 CPU-s of a debug process's start (parse included),
/// far inside the 3 s deadline.
#[test]
fn a_budget_cut_after_a_depth_cap_hit_says_both_remedies_may_help() {
    let f = Fixture::forall("hit", false);
    let n = Fixture::node_chain("hit-nodes");
    let cases: [Case<'_>; 3] = [
        (
            "F1 armed, not latched",
            f.path(),
            vec![
                ("RUSTDL_TABLEAU_EARLY_ABANDON_HITS", "1000000"),
                ("RUSTDL_REALIZE_PAIR_TIMEOUT_MS", "3000"),
            ],
        ),
        (
            "F2 early abandon off",
            f.path(),
            vec![
                ("RUSTDL_TABLEAU_EARLY_ABANDON", "0"),
                ("RUSTDL_REALIZE_PAIR_TIMEOUT_MS", "3000"),
            ],
        ),
        (
            "F3 node cap after a hit",
            n.path(),
            vec![
                ("RUSTDL_TABLEAU_EARLY_ABANDON_HITS", "1000000"),
                ("RUSTDL_REALIZE_PAIR_TIMEOUT_MS", LONG_DEADLINE_MS),
                ("RUSTDL_QUERY_MAX_NODES", "20"),
            ],
        ),
    ];
    for (case, p, env) in &cases {
        let t = Instant::now();
        let r = run(env, &["instance", p, Y, A]);
        if case.starts_with("F3") {
            // Far inside the 60 s deadline: the node cap ended it.
            assert!(
                t.elapsed() < Duration::from_secs(30),
                "[{case}] {:?}",
                t.elapsed()
            );
        }
        assert_eq!(r.code, 3, "[{case}] {}\n{}", r.stdout, r.stderr);
        assert_eq!(r.stdout.trim(), "unknown", "[{case}]");
        assert!(
            r.stderr
                .contains("instance probe was cut (deadline or node cap)"),
            "[{case}] {}",
            r.stderr
        );
        assert!(
            r.stderr
                .contains("It had hit the tableau's decision-depth limit")
                && r.stderr.contains("may help as well as a larger budget"),
            "[{case}] {}",
            r.stderr
        );
        assert!(
            !r.stderr.contains("does not help"),
            "[{case}] not the depth_capped wording: {}",
            r.stderr
        );

        let r = run(env, &["instances", p, Y]);
        assert_eq!(r.code, 0, "[{case}] {}\n{}", r.stdout, r.stderr);
        assert!(
            r.stderr
                .contains("at least one instance probe was cut (deadline or node cap)")
                && r.stderr
                    .contains("had hit the tableau's decision-depth limit")
                && r.stderr.contains("may help as well"),
            "[{case}] {}",
            r.stderr
        );
        assert!(!r.stderr.contains("does not help"), "[{case}] {}", r.stderr);
    }
}

/// #510 (C) is report-only and leaves classify alone: a classify pair probe cut by its
/// deadline after depth-cap hits (F1's setting) is a plain timed-out pair, not one of
/// #424's `depth_capped_pairs`, whose banner says a larger budget does not help.
#[test]
fn classify_does_not_count_a_depth_cap_hit_as_a_depth_cut() {
    let f = Fixture::forall("classify-hit", false);
    let env = [("RUSTDL_TABLEAU_EARLY_ABANDON_HITS", "1000000")];
    let r = run(&env, &["classify", "--pair-timeout-ms", "3000", f.path()]);
    assert_eq!(r.code, 0, "{}\n{}", r.stdout, r.stderr);
    assert!(r.stderr.contains("INCOMPLETE"), "a cut pair: {}", r.stderr);
    assert!(!r.stderr.contains("search-depth limit"), "{}", r.stderr);
    // Control: at the default early abandon the same pair IS depth-capped.
    let r = run(&[], &["classify", "--pair-timeout-ms", "3000", f.path()]);
    assert!(r.stderr.contains("search-depth limit"), "{}", r.stderr);
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
