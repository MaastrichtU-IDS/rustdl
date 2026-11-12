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
