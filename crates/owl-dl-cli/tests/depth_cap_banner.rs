//! #288: the INCOMPLETE banner reports a probe cut by the tableau's decision-depth
//! limit as such, not as "hit the N ms per-pair timeout", and does not suggest
//! that a larger budget would help.
#![allow(clippy::unwrap_used)]

use std::fmt::Write as _;
use std::process::Command;

/// `X ⊑ ⊓ₖ (Aₖ ⊔ Bₖ)` over `n` independent unions: `X`'s unsat probe needs `n`
/// decision levels, past the limit (256) for `n = 280`.
fn classify_stderr(n: usize) -> String {
    let mut src =
        String::from("Prefix(:=<http://example.org/d#>)\nOntology(<http://example.org/d>\n");
    src.push_str("SubClassOf(:X ObjectIntersectionOf(");
    for k in 0..n {
        write!(src, " ObjectUnionOf(:A{k} :B{k})").unwrap();
    }
    src.push_str("))\n)\n");
    let path =
        std::env::temp_dir().join(format!("rustdl-depth-cap-{}-{n}.ofn", std::process::id()));
    std::fs::write(&path, src).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_rustdl"))
        .args(["classify", "--pair-timeout-ms", "20000"])
        .arg(&path)
        .output()
        .unwrap();
    std::fs::remove_file(&path).ok();
    assert!(out.status.success());
    String::from_utf8_lossy(&out.stderr).into_owned()
}

#[test]
fn a_depth_limit_cut_is_not_called_a_timeout() {
    let err = classify_stderr(280);
    assert!(err.contains("INCOMPLETE"), "{err}");
    assert!(
        err.contains("hit the tableau's search-depth limit (256 decision levels)"),
        "{err}"
    );
    assert!(!err.contains("per-pair timeout"), "{err}");
}

#[test]
fn within_the_depth_limit_there_is_no_banner() {
    let err = classify_stderr(200);
    assert!(!err.contains("INCOMPLETE"), "{err}");
}
