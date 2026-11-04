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

fn classify_json(probe_ms: &str) -> serde_json::Value {
    let path: PathBuf = std::env::temp_dir().join(format!(
        "rustdl-probe-giveup-{}-{probe_ms}.ofn",
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
        .args(["classify", "--json", path.to_str().unwrap()])
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

#[test]
fn a_cut_consistency_probe_is_reported_as_undetermined() {
    let j = classify_json("0");
    assert_eq!(j["consistent"], true);
    assert_eq!(j["consistency_undetermined"], true);
    assert_eq!(j["incomplete"], false);
    // With room to finish, the probe finds the clash.
    let j = classify_json("30000");
    assert_eq!(j["consistent"], false);
}
