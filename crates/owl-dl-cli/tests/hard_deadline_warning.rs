//! #162: the CLI names a CONVERSION cut as such, not as a partial saturation.

#![allow(clippy::unwrap_used)]

use std::fmt::Write as _;
use std::process::Command;

fn classify_stderr(hard: &str, convert: &str) -> String {
    let mut src = String::from("Prefix(:=<http://ex#>)\nOntology(\n");
    for i in 0..3000 {
        writeln!(src, "SubClassOf(:A{i} ObjectSomeValuesFrom(:r :B{i}))").unwrap();
    }
    src.push(')');
    let path = std::env::temp_dir().join(format!(
        "rustdl-conv-cut-{}-{hard}{convert}.ofn",
        std::process::id()
    ));
    std::fs::write(&path, src).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_rustdl"))
        .args(["classify", "--global-timeout-ms", "1"])
        .arg(&path)
        .env("RUSTDL_PREP_DEADLINE", "1")
        .env("RUSTDL_HARD_GLOBAL_DEADLINE", hard)
        .env("RUSTDL_CONVERT_DEADLINE", convert)
        .output()
        .unwrap();
    std::fs::remove_file(&path).ok();
    assert!(out.status.success());
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// A 1 ms budget is spent by parsing, so hard mode cuts conversion at once.
#[test]
fn a_conversion_cut_gets_its_own_warning() {
    let err = classify_stderr("1", "1");
    assert!(err.contains("during conversion"), "{err}");
    assert!(!err.contains("during preparation"), "{err}");
}

/// With conversion let finish, the same run is a preparation cut.
#[test]
fn a_preparation_cut_keeps_its_warning() {
    let err = classify_stderr("1", "0");
    assert!(err.contains("during preparation"), "{err}");
    assert!(!err.contains("during conversion"), "{err}");
}
