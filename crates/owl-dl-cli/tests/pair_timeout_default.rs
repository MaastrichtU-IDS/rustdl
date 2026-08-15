//! The `classify --pair-timeout-ms` default is **5 ms** as of v0.4.19.
//!
//! Pinned because the value is load-bearing and was chosen by measurement, not taste:
//! a 1,920-ontology two-arm sweep found 16 recoveries, 0 regressions and −15.9% wall,
//! with ΔMISSED +8 (+1.04%) and FP=0 over a 400-ontology Konclude ∪ `HermiT` net.
//! **1 ms was screened and rejected** at ΔMISSED +360 (+46.75%), so the completeness
//! cliff sits between 1 and 5 and this default should not drift downward casually.
//!
//! Only `classify` moved. `disjoint`, `individuals` and `property-values` keep their
//! own 1000 ms defaults — the sweep measured `classify`, and flipping the others
//! would be unmeasured.

use std::process::Command;

fn rustdl() -> &'static str {
    env!("CARGO_BIN_EXE_rustdl")
}

#[test]
fn classify_help_states_the_five_millisecond_default() {
    let out = Command::new(rustdl())
        .args(["classify", "--help"])
        .output()
        .expect("run rustdl classify --help");
    let text = String::from_utf8_lossy(&out.stdout);
    let line = text
        .lines()
        .find(|l| l.contains("--pair-timeout-ms"))
        .unwrap_or_default()
        .to_string();
    // clap renders `[default: N]` for `default_value_t`.
    assert!(
        text.contains("[default: 5]"),
        "classify --pair-timeout-ms must default to 5 ms (v0.4.19); help line was: {line}\n{text}"
    );
}

/// The other subcommands were deliberately NOT changed. If this fails because someone
/// lowered them too, that change needs its own sweep — `classify`'s does not cover it.
#[test]
fn other_subcommands_keep_their_own_default() {
    for sub in ["disjoint", "individuals", "property-values"] {
        let out = Command::new(rustdl())
            .args([sub, "--help"])
            .output()
            .unwrap_or_else(|e| panic!("run rustdl {sub} --help: {e}"));
        let text = String::from_utf8_lossy(&out.stdout);
        if text.contains("--pair-timeout-ms") {
            assert!(
                text.contains("[default: 1000]"),
                "{sub} --pair-timeout-ms must still default to 1000 (only classify moved)"
            );
        }
    }
}
