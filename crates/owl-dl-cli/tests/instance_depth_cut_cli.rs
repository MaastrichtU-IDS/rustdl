//! #510: an `instance` probe cut by the tableau's decision-depth limit is reported as
//! such, and the advice says what recovers it.
//!
//! The fixture is #288's 280-union: `a : X` with `X ⊑ ⊓ₖ (Aₖ ⊔ Bₖ)`, `k < 280`, and an
//! unrelated `Y`. `a : Y` is not entailed, but the bounded probe (`{a} ⊓ ¬Y` under the
//! default 750 ms per-pair deadline) needs 280 decision levels, past `MAX_SEARCH_DEPTH`
//! (256). Every branch hits the cap and early abandon gives up after 32 hits, in well under
//! the deadline: the census's ORE shape (#510: exactly 32 cap hits per trace). A larger
//! timeout or node cap would not change that, and the old message advised both. The
//! deadline-free probe (`RUSTDL_REALIZE_PAIR_TIMEOUT_MS=0`) searches deeper and answers.
#![allow(clippy::unwrap_used)]

use std::fmt::Write as _;
use std::process::Command;

struct Out {
    code: i32,
    stdout: String,
    stderr: String,
}

fn fixture(tag: &str) -> std::path::PathBuf {
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
    path
}

/// Runs `rustdl` with the probe settings at their defaults unless `env` sets them.
fn run(env: &[(&str, &str)], args: &[&str]) -> Out {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_rustdl"));
    for var in [
        "RUSTDL_REALIZE_PAIR_TIMEOUT_MS",
        "RUSTDL_TABLEAU_EARLY_ABANDON",
        "RUSTDL_MAX_NODES",
        "RUSTDL_QUERY_MAX_NODES",
    ] {
        cmd.env_remove(var);
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

#[test]
fn a_depth_cut_instance_probe_names_the_limit_and_the_advice_that_helps() {
    let f = fixture("default");
    let p = f.to_str().unwrap();
    let r = run(&[], &["instance", p, Y, A]);
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
    let r = run(&[], &["instances", p, Y]);
    assert!(
        r.stderr.contains("hit the tableau's decision-depth limit"),
        "{}",
        r.stderr
    );
    std::fs::remove_file(&f).ok();
}

#[test]
fn a_deadline_cut_instance_probe_keeps_the_budget_advice() {
    // Early abandon off and a 1 ms per-pair deadline: the same probe now ends on the
    // deadline, before 280 decision levels, so the cut is NOT depth-capped and a larger
    // budget is the right advice.
    let f = fixture("deadline");
    let p = f.to_str().unwrap();
    let r = run(
        &[
            ("RUSTDL_TABLEAU_EARLY_ABANDON", "0"),
            ("RUSTDL_REALIZE_PAIR_TIMEOUT_MS", "1"),
        ],
        &["instance", p, Y, A],
    );
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
    std::fs::remove_file(&f).ok();
}
