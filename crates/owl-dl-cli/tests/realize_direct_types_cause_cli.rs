//! #583: when #388's co-leaf check leaves two direct types undecided, `realize` says
//! what may decide them: the check's cut cause (`direct_types_depth_capped` in JSON) and
//! the remedy in the text warning.
//!
//! Fixture: #510 C's pigeonhole, one size up (10 pigeons, 9 holes, forced only under
//! `¬Y`), with `a : X` and `a : Y` told. `X ⊑ Y` is entailed, but refuting `X ⊓ ¬Y` must
//! cover every placement. #595: the tests run with `RUSTDL_REALIZE_PAIR_TIMEOUT_MS=1`,
//! which bounds both realize's classification step and the co-leaf check, so the cut
//! does not depend on how fast the host is (at the default 750 ms a fast enough build
//! could decide it). The search depth is at most 10, so the cut is the deadline, never
//! the depth limit: `types` is `{X, Y}` either way, and `direct_types` keeps the
//! superclass `Y`, flagged.
//!
//! Not pinned: a depth-capped co-leaf check (no fixture found; see #583's measurement),
//! and the no-advice branch for a deadline-free run or a likely-bug `NoVerdict` (a
//! deadline-free run of this fixture does not finish in test time).
#![allow(clippy::unwrap_used)]

use std::fmt::Write as _;
use std::path::PathBuf;
use std::process::Command;

struct Fixture(PathBuf);

impl Fixture {
    /// `tag` keeps each test's file apart: the tests run in parallel, and each drop
    /// removes its file.
    fn pigeonhole_with_y(tag: &str) -> Self {
        const HOLES: usize = 9;
        let p = |i: usize, j: usize| format!(":P{i}_{j}");
        let holes = |i: usize| (0..HOLES).map(|j| p(i, j)).collect::<Vec<_>>().join(" ");
        let mut src = String::from(
            "Prefix(owl:=<http://www.w3.org/2002/07/owl#>)\nPrefix(:=<http://example.org/d#>)\n\
             Ontology(<http://example.org/d>\n\
             Declaration(NamedIndividual(:a)) Declaration(Class(:Y)) Declaration(Class(:X))\n",
        );
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
        src.push_str("ClassAssertion(:X :a)\nClassAssertion(:Y :a)\n)\n");
        let path = std::env::temp_dir().join(format!(
            "rustdl-realize-direct-cause-{}-{tag}.ofn",
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

/// `rustdl` with every `RUSTDL_*` variable cleared but `env`: `(stdout, stderr)`.
fn run(env: &[(&str, &str)], args: &[&str]) -> (String, String) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_rustdl"));
    for (var, _) in std::env::vars_os() {
        if var.to_string_lossy().starts_with("RUSTDL_") {
            cmd.env_remove(var);
        }
    }
    let out = cmd.envs(env.iter().copied()).args(args).output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// A 1 ms per-pair budget: no host decides the fixture within it (#595).
const CUT: &[(&str, &str)] = &[("RUSTDL_REALIZE_PAIR_TIMEOUT_MS", "1")];

const UNDECIDED: &str = "some individual has two direct types whose subsumption was left undecided";

#[test]
fn a_deadline_cut_check_names_the_budget_as_the_remedy() {
    let f = Fixture::pigeonhole_with_y("deadline");
    let (out, _) = run(CUT, &["realize", "--json", f.path()]);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["direct_types_unverified"], true, "{v}");
    assert_eq!(v["direct_types_depth_capped"], false, "{v}");
    let (_, err) = run(CUT, &["realize", f.path()]);
    assert!(err.contains(UNDECIDED), "{err}");
    assert!(
        err.contains(
            "For a deadline or node-cap cut, a larger RUSTDL_REALIZE_PAIR_TIMEOUT_MS or \
             RUSTDL_MAX_NODES"
        ),
        "{err}"
    );
    assert!(!err.contains("decision-depth limit"), "{err}");
}

#[test]
fn a_check_that_did_not_run_says_why() {
    let f = Fixture::pigeonhole_with_y("not-run");
    let (_, err) = run(
        &[("RUSTDL_REALIZE_VERIFY_DIRECT", "0"), CUT[0]],
        &["realize", f.path()],
    );
    assert!(err.contains(UNDECIDED), "{err}");
    assert!(
        err.contains("The check is off (RUSTDL_REALIZE_VERIFY_DIRECT=0)"),
        "{err}"
    );
    assert!(
        !err.contains("a larger RUSTDL_REALIZE_PAIR_TIMEOUT_MS"),
        "{err}"
    );
    // The saturation-only realize cannot prove X ⊑ Y either, and runs no check.
    let (_, err) = run(&[], &["realize", "--saturation-only", f.path()]);
    assert!(err.contains(UNDECIDED), "{err}");
    assert!(err.contains("--saturation-only runs no check"), "{err}");
    assert!(
        !err.contains("a larger RUSTDL_REALIZE_PAIR_TIMEOUT_MS"),
        "{err}"
    );
}
