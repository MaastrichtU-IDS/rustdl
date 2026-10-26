//! #288 step 4e: the `RUSTDL_TRACE_DECISIONS=1` trace that the merge-bar
//! measurement compares across two processes (plan 4e, revision 4e-1). Its
//! decision identities must not depend on anything a second run of the same
//! binary on the same input could change, and the trace must print nothing
//! when the variable is unset.

#![allow(clippy::unwrap_used)]

use std::process::Command;

fn fixture(name: &str) -> String {
    format!(
        "{}/tests/fixtures/selective/{name}",
        env!("CARGO_MANIFEST_DIR")
    )
}

/// The stderr of `rustdl sat FILE Q`, single-threaded, node cap off.
fn sat_stderr(name: &str, env: &[(&str, &str)]) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_rustdl"))
        .args(["sat", &fixture(name), "http://x.org/f#Q"])
        .env("RUSTDL_MAX_NODES", "0")
        .env("RAYON_NUM_THREADS", "1")
        .envs(env.iter().copied())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "rustdl sat {name} exited {}",
        out.status
    );
    String::from_utf8(out.stderr).unwrap()
}

/// The `# dec` lines, which carry no timing.
fn decisions(stderr: &str) -> Vec<&str> {
    stderr.lines().filter(|l| l.starts_with("# dec ")).collect()
}

#[test]
fn two_runs_trace_the_same_decisions() {
    for name in [
        "stale_merge_frame_mark.ofn",
        "restored_edge_on_merged_source.ofn",
    ] {
        for flag in ["0", "1"] {
            let env = [
                ("RUSTDL_TRACE_DECISIONS", "1"),
                ("RUSTDL_SELECTIVE_UNDO", flag),
            ];
            let first = sat_stderr(name, &env);
            let second = sat_stderr(name, &env);
            let decs = decisions(&first);
            assert!(!decs.is_empty(), "{name}: no decision traced");
            assert_eq!(decs, decisions(&second), "{name}, flag {flag}");
            assert!(
                first
                    .lines()
                    .any(|l| l.starts_with("# search start cpu_ns=")),
                "{name}: no start line"
            );
        }
    }
}

#[test]
fn the_trace_prints_nothing_unless_asked() {
    let stderr = sat_stderr(
        "stale_merge_frame_mark.ofn",
        &[("RUSTDL_SELECTIVE_UNDO", "1")],
    );
    assert!(
        !stderr.lines().any(|l| l.starts_with("# dec ")
            || l.starts_with("# jump ")
            || l.starts_with("# search start")),
        "decision trace printed with RUSTDL_TRACE_DECISIONS unset"
    );
}

/// The `# jump` lines with every skip shown (`RUSTDL_TRACE_DECISIONS_MIN_JUMP=1`).
fn jumps(name: &str, flag: &str) -> Vec<String> {
    let env = [
        ("RUSTDL_TRACE_DECISIONS", "1"),
        ("RUSTDL_TRACE_DECISIONS_MIN_JUMP", "1"),
        ("RUSTDL_SELECTIVE_UNDO", flag),
    ];
    sat_stderr(name, &env)
        .lines()
        .filter(|l| l.starts_with("# jump "))
        .map(str::to_owned)
        .collect()
}

/// The value of `key=` on a trace line.
fn field<'a>(line: &'a str, key: &str) -> &'a str {
    line.split_whitespace()
        .find_map(|w| w.strip_prefix(key)?.strip_prefix('='))
        .unwrap_or_else(|| panic!("no {key} in {line}"))
}

/// The jump's identity and depth, without its timing.
fn jump_key(line: &str) -> String {
    ["kind", "set", "max", "opt", "chosen", "skipped"]
        .iter()
        .filter(|k| line.contains(&format!(" {k}=")))
        .map(|k| format!("{k}={}", field(line, k)))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Plan 4e: with the flag off nothing is kept, so `skipped_eff` is `skipped`;
/// with it on, the selective backjump says what it kept. The first jump is
/// the same decision at the same depth in both arms.
#[test]
fn jump_lines_count_kept_frames_only_with_the_flag_on() {
    let name = "stale_merge_frame_mark.ofn";
    let off = jumps(name, "0");
    let on = jumps(name, "1");
    assert!(!off.is_empty() && !on.is_empty(), "no jump traced");
    for line in &off {
        assert_eq!(field(line, "kept_below"), "0", "{line}");
        assert_eq!(field(line, "skipped_eff"), field(line, "skipped"), "{line}");
        assert_eq!(
            (field(line, "sel"), field(line, "kept")),
            ("0", "0"),
            "{line}"
        );
    }
    assert!(
        on.iter()
            .any(|l| field(l, "sel") == "1" && field(l, "kept").parse::<usize>().unwrap() > 0),
        "no selective jump that kept frames: {on:?}"
    );
    assert_eq!(jump_key(&off[0]), jump_key(&on[0]));
}
