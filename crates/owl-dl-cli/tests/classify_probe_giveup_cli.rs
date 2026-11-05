//! #446: when classify's consistency probe is admitted and gives up, `classify --json`
//! reports `consistency_undetermined: true`. `incomplete` stays false: it means only
//! that a pair deadline fired, and the probe's budget is not one.
#![allow(clippy::unwrap_used)]

use std::path::PathBuf;
use std::process::Command;

/// #437 reproducer B: `TBox` only, inconsistent (Konclude and `HermiT`); classify's
/// probe is admitted because four classes are unsatisfiable.
const REPRO_B: &str = "Prefix(:=<http://gen.rustdl/o#>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
Ontology(<http://gen.rustdl/o>
Declaration(Class(:C1)) Declaration(Class(:C2)) Declaration(Class(:C4)) Declaration(Class(:C5))
Declaration(ObjectProperty(:r0)) Declaration(ObjectProperty(:r1))
SubClassOf(ObjectSomeValuesFrom(:r1 owl:Thing) :C4)
DisjointUnion(:C2 :C1 :C4)
SubClassOf(:C5 :C4)
EquivalentClasses(:C5 ObjectAllValuesFrom(:r1 ObjectUnionOf(:C5 :C2)))
SubClassOf(:C2 ObjectSomeValuesFrom(:r0 ObjectHasSelf(ObjectInverseOf(:r1))))
EquivalentClasses(:C1 ObjectSomeValuesFrom(:r0 :C4))
)
";

/// `classify --json` on reproB with no inherited `RUSTDL_*` setting, the probe
/// budget `probe_ms`, plus `env` and `args`. `tag` keeps parallel tests' temp files
/// apart.
fn classify_json(
    tag: &str,
    probe_ms: &str,
    env: &[(&str, &str)],
    args: &[&str],
) -> serde_json::Value {
    let path: PathBuf = std::env::temp_dir().join(format!(
        "rustdl-probe-giveup-{}-{tag}-{probe_ms}.ofn",
        std::process::id()
    ));
    std::fs::write(&path, REPRO_B).unwrap();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_rustdl"));
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("RUSTDL_") {
            cmd.env_remove(key);
        }
    }
    let out = cmd
        .env("RUSTDL_CLASSIFY_CONSISTENCY_PROBE_MS", probe_ms)
        .envs(env.iter().copied())
        .arg("classify")
        .arg("--json")
        .args(args)
        .arg(&path)
        .output()
        .unwrap();
    let _ = std::fs::remove_file(&path);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}

/// Unbounded pair and global budgets: every unsat probe finishes, so the probe is
/// admitted on the four unsat classes and `incomplete` is false whatever the build's
/// speed (#466: with the default 5 ms pair budget a slow debug build sometimes cut
/// the unsat probes, and this test flaked).
const NO_CUTS: &[&str] = &["--pair-timeout-ms", "0", "--global-timeout-ms", "0"];

#[test]
fn a_cut_consistency_probe_is_reported_as_undetermined() {
    let j = classify_json("nocut", "0", &[], NO_CUTS);
    assert_eq!(j["consistent"], true);
    assert_eq!(j["consistency_undetermined"], true);
    assert_eq!(j["incomplete"], false);
    // With room to finish, the probe finds the clash.
    let j = classify_json("nocut", "30000", &[], NO_CUTS);
    assert_eq!(j["consistent"], false);
}

/// #466: with every per-class unsat probe cut (a 2-node tableau cap, the wedge off),
/// no class is found unsatisfiable, and before #466 the probe was not admitted on a
/// `TBox`-only KB: `consistent: true` with `consistency_undetermined: false` on an
/// inconsistent ontology. Now a cut unsat probe admits it. Its layer 3 runs under
/// the same node cap and gives up, so the verdict is flagged undetermined.
#[test]
fn cut_unsat_probes_on_a_tbox_only_kb_leave_consistency_undetermined() {
    let cut = [("RUSTDL_MAX_NODES", "2"), ("RUSTDL_HYPERTABLEAU", "0")];
    for probe_ms in ["0", "200"] {
        let j = classify_json("cut", probe_ms, &cut, &[]);
        // The precondition: the unsat probes really were cut.
        assert_eq!(j["incomplete"], true, "{probe_ms}: {j}");
        assert_eq!(j["unsatisfiable"], serde_json::json!([]), "{probe_ms}: {j}");
        assert!(
            j["consistent"] == false || j["consistency_undetermined"] == true,
            "{probe_ms}: an inconsistent KB read as determined-consistent: {j}"
        );
    }
    // A zero probe budget can only give up.
    let j = classify_json("cut0", "0", &cut, &[]);
    assert_eq!(j["consistency_undetermined"], true, "{j}");
}
