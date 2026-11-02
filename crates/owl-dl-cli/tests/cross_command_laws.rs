//! #363 package 2: rustdl's own commands must agree on equivalent questions.
//!
//! A local gate, like `per_pair_fp_gate.rs`: it runs the quick mode of
//! `scripts/cross-command-laws.py` against this crate's `rustdl` binary. The
//! script first runs its `--self-test` (each law fires on fabricated operands,
//! and an unknown operand never counts as a violation), then the quick corpus
//! (pizza, ro, sulo, bibtex and two synthetics), and fails on a hard-law
//! violation or on a ratchet count above `tests/fixtures/laws/baseline.json`.
//!
//! Coverage, stated: no hard law has a live engine-level sabotage on the curated
//! fixtures; the self-test checks the harness logic, not the engine. The
//! engine-level sabotages (R2, R3) run with `--sabotage`.
//!
//! Run: `cargo test --release -p owl-dl-cli --test cross_command_laws -- --ignored`.
//! Needs `python3` and the gitignored fixtures (`scripts/fetch-real-ontologies.sh`);
//! about 5 minutes on 3 cores. `RUSTDL_LAWS_CORES=92,93,94` pins the workers.

use std::path::PathBuf;
use std::process::Command;

fn script() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../scripts/cross-command-laws.py")
}

#[test]
#[ignore = "local gate: ~5 min on 3 cores, needs python3 and the gitignored fixtures"]
fn cross_command_laws_quick() {
    let st = Command::new("python3")
        .arg(script())
        .arg("--self-test")
        .status()
        .expect("python3");
    assert!(st.success(), "harness self-test failed");

    let out = std::env::temp_dir().join(format!("rustdl-laws-{}", std::process::id()));
    let baseline =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/laws/baseline.json");
    let mut cmd = Command::new("python3");
    cmd.arg(script())
        .args(["--bin", env!("CARGO_BIN_EXE_rustdl"), "--quick", "--out"])
        .arg(&out)
        .arg("--baseline")
        .arg(&baseline);
    if let Ok(cores) = std::env::var("RUSTDL_LAWS_CORES") {
        cmd.args(["--cores", &cores]);
    }
    let st = cmd.status().expect("python3");
    assert!(
        st.success(),
        "cross-command law violation or ratchet regression; see {}",
        out.join("report.json").display()
    );
}
