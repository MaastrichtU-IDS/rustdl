//! End-to-end tests for `rustdl verify-el` — the `owl-dl-verify` instrument
//! wired up as a CLI subcommand.
//!
//! Exit codes are the load-bearing contract here (a corpus sweep buckets
//! outcomes from the exit code alone, without parsing stdout): **0**
//! `Verified`, **2** `Violated`, **3** `Unresolved`, **1** I/O/parse errors.
//! Fixtures are reused directly from `crates/owl-dl-verify/tests/fixtures/`
//! (their verdicts are already established and reasoned through by that
//! crate's own test suite — see `.superpowers/sdd/2026-08-28-negative-
//! certificates-phase1/task-12-report.md`'s coverage table) rather than
//! duplicated here.

#![allow(clippy::unwrap_used)]

use std::process::Command;

fn rustdl() -> Command {
    Command::new(env!("CARGO_BIN_EXE_rustdl"))
}

/// A fixture already established (by `owl-dl-verify`'s own test suite) to
/// converge to `Verified`: `unsatconj.ofn` — `X ⊑ ∃r.Y`, `Range(r,F)`,
/// `DisjointClasses(Y,F)`, so `X` is unsatisfiable and gets no element; the
/// three written axioms all hold vacuously or directly over the resulting
/// model.
fn verified_fixture() -> &'static str {
    concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../owl-dl-verify/tests/fixtures/unsatconj.ofn"
    )
}

/// A fixture that lands on `Violated` for a `owl-dl-verify` BUILDER reason
/// (`known_limitations.rs`'s F1: a conjunctive `∃`-body plus a GCI over that
/// conjunction — the witness label is never closed under `A ⊓ B ⊑ C`), NOT an
/// rustdl engine defect.
///
/// This used to be `chainpoison.ofn`, one of the acceptance suite's real,
/// still-open rustdl *engine* completeness defects — but that defect is
/// exactly the one issue #80/#82's saturator fix (the `Some`/`Min` one-way-
/// marker bug in `atomic_or_tseitin_body_with_extras`) closed, so
/// `chainpoison.ofn` now verifies cleanly and no longer exercises this exit
/// path. Do NOT repoint this at another rustdl-engine-defect fixture
/// (`chain-range-bot.ofn`, `unsatnested.ofn`, …) — those are exactly the kind
/// of thing future engine fixes keep closing, which is what broke this test
/// in the first place. A builder-defect fixture is stable under engine
/// changes because the two are different crates' code.
fn violated_fixture() -> &'static str {
    concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../owl-dl-verify/tests/fixtures/conjexists-builder-gap.ofn"
    )
}

/// A fixture `build_model` REFUSES outright (`Err(ChainRangeOutOfProfile)`),
/// never reaching `verify`'s check loop at all — the other, harder-to-reach
/// way to land on `Unresolved` besides the fragment gate.
fn build_refused_fixture() -> &'static str {
    concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../owl-dl-verify/tests/fixtures/chainrange.ofn"
    )
}

/// A fixture with build-time `LabelNotClosed` residue ALONGSIDE a `Violated`
/// check verdict (`cascade.ofn`) — used for the determinism check because it
/// exercises both output channels (`violations` and `unresolved`) at once.
fn cascade_fixture() -> &'static str {
    concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../owl-dl-verify/tests/fixtures/cascade.ofn"
    )
}

/// Non-empty `build_model` reasons paired with a model that still satisfies
/// every WRITTEN axiom, i.e. a `Verified` CHECK result. This is the ONLY
/// fixture in this suite that exercises `fold_build_reasons`'s
/// `Verified -> Unresolved` downgrade arm — the single most safety-critical
/// line in `main.rs`, since an accidental pass-through there would report
/// exit 0 over an admitted, unclosed gap.
///
/// WAS `markerresidue.ofn`, whose residue was a SATURATOR DEFECT (nested
/// existential bodies lowered without folding `ObjectPropertyRange`). Issue
/// #81 fixed it and this test failed — correctly, and loudly. `topwitness.ofn`
/// is durable instead: `A ⊑ ∃u.⊤` lowers to a deliberately subsumer-less
/// ⊤-witness, so `Range(u)` cannot be folded into it WITHOUT breaking the
/// domain inference that witness exists for. That gap is a documented design
/// decision, not a bug waiting to be closed. See
/// `crates/owl-dl-verify/tests/model.rs`'s
/// `verified_check_can_still_carry_nonempty_build_reasons`.
fn residue_fixture() -> &'static str {
    concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../owl-dl-verify/tests/fixtures/topwitness.ofn"
    )
}

/// A fixture with ONE unsupported `HasKey` axiom (dropped gracefully at
/// conversion per `CLAUDE.md`'s "Graceful degradation + surfaced drops"
/// entry) alongside an otherwise-trivial pure-EL `SubClassOf(:A :B)`. Found
/// during the final whole-branch review: before `fold_dropped_axioms`
/// existed, this exited **0** `Verified` — the checker vouched for a
/// closure built from strictly fewer axioms than the ontology actually has,
/// with the drop reported only as a stderr warning a corpus sweep bucketing
/// on exit code alone would never see.
fn dropped_axiom_fixture() -> &'static str {
    concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/dropped-haskey.ofn"
    )
}

/// `ontologies/real/pizza.ofn` is part of the gitignored corpus
/// (`./scripts/fetch-real-ontologies.sh` pulls it on demand) — present in
/// this sandbox, but a fresh clone or CI checkout may not have it. Every
/// test that depends on it skips (rather than fails) when it is absent,
/// following the established in-tree convention (e.g.
/// `crates/owl-dl-reasoner/tests/data_properties.rs`).
fn pizza_path() -> std::path::PathBuf {
    std::path::PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../ontologies/real/pizza.ofn"
    ))
}

#[test]
fn verified_exits_zero() {
    let out = rustdl()
        .arg("verify-el")
        .arg(verified_fixture())
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.starts_with("verified:"),
        "expected a verified: line, got {stdout:?}"
    );
}

#[test]
fn violated_exits_two() {
    let out = rustdl()
        .arg("verify-el")
        .arg(violated_fixture())
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(2),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.starts_with("violated:"),
        "expected a violated: line, got {stdout:?}"
    );
}

#[test]
fn build_refusal_is_unresolved_and_exits_three() {
    let out = rustdl()
        .arg("verify-el")
        .arg(build_refused_fixture())
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(3),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.starts_with("unresolved:"),
        "expected an unresolved: line, got {stdout:?}"
    );
    assert!(
        stdout.contains("ChainRangeOutOfProfile"),
        "the build-time refusal reason must be surfaced, not swallowed: {stdout:?}"
    );
}

/// The bug this test guards: a fixture that would otherwise be `Verified`
/// must be downgraded to `Unresolved` (exit 3) when the converter silently
/// dropped a content axiom `verify-el` never got to check — see
/// `fold_dropped_axioms` in `main.rs`.
#[test]
fn dropped_axioms_downgrade_a_verified_check_to_unresolved_and_exit_three() {
    let out = rustdl()
        .arg("verify-el")
        .arg(dropped_axiom_fixture())
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(3),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.starts_with("unresolved:"),
        "expected an unresolved: line, got {stdout:?}"
    );
    assert!(
        stdout.contains("AxiomsDroppedAtConversion"),
        "the dropped-axiom reason must be surfaced, not swallowed: {stdout:?}"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("HasKey"),
        "warn_if_dropped's own warning must still fire: {stderr:?}"
    );
}

/// Same fixture, `--json`: the `dropped` block must still be present (it
/// always is, at any verdict) AND the top-level `verdict` must say
/// `"unresolved"`, not `"verified"`.
#[test]
fn dropped_axioms_downgrade_is_visible_in_json_too() {
    let out = rustdl()
        .arg("verify-el")
        .arg("--json")
        .arg(dropped_axiom_fixture())
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(3),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON on stdout");
    assert_eq!(json["verdict"], "unresolved");
    assert_eq!(json["dropped"]["HasKey: unsupported axiom (HasKey)"], 1);
}

/// `verify-el ontologies/real/pizza.ofn` must be `Unresolved` (exit 3):
/// pizza is a SROIQ ontology (nominals, cardinality, disjunction), so
/// `analyze_fragment` reads `OutOfFragment`, and the CLI must refuse it
/// BEFORE ever calling `build_model` — never `Verified` (this checker has
/// no basis at all for a verdict on an out-of-fragment ontology) and never
/// silently falling through to some other exit code.
#[test]
fn out_of_fragment_ontology_is_unresolved_and_exits_three() {
    let path = pizza_path();
    if !path.exists() {
        eprintln!("SKIP: missing corpus fixture {}", path.display());
        return;
    }
    let out = rustdl().arg("verify-el").arg(&path).output().unwrap();
    assert_eq!(
        out.status.code(),
        Some(3),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.starts_with("unresolved:"),
        "expected an unresolved: line, got {stdout:?}"
    );
    assert!(
        stdout.contains("not pure-EL"),
        "the fragment-gate reason must be surfaced: {stdout:?}"
    );
}

#[test]
fn missing_file_is_an_io_error_and_exits_one() {
    let out = rustdl()
        .arg("verify-el")
        .arg("/does/not/exist/nowhere.ofn")
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(1),
        "stdout: {}",
        String::from_utf8_lossy(&out.stdout)
    );
    assert!(
        !out.stderr.is_empty(),
        "an I/O error must be reported on stderr"
    );
}

#[test]
fn malformed_ontology_is_a_parse_error_and_exits_one() {
    let dir = std::env::temp_dir();
    let path = dir.join(format!("verify-el-malformed-{}.ofn", std::process::id()));
    std::fs::write(&path, b"this is not valid OWL functional syntax (((").unwrap();
    let out = rustdl().arg("verify-el").arg(&path).output().unwrap();
    let _ = std::fs::remove_file(&path);
    assert_eq!(
        out.status.code(),
        Some(1),
        "stdout: {}",
        String::from_utf8_lossy(&out.stdout)
    );
    assert!(
        !out.stderr.is_empty(),
        "a parse error must be reported on stderr"
    );
}

/// The determinism requirement: rustdl shipped exactly this bug before in
/// `justify`/`report` (issue #59) because `FiniteModel` (like the structures
/// behind those commands) is built over hash maps whose iteration order is
/// not guaranteed stable across separate process invocations. Runs the
/// command as two entirely separate child processes (not two calls within
/// one process) — that is the case issue #59 actually hit, since each
/// invocation of the binary seeds its own `RandomState`.
#[test]
fn json_output_is_byte_identical_across_two_separate_process_runs() {
    let run = || -> Vec<u8> {
        rustdl()
            .arg("verify-el")
            .arg("--json")
            .arg(cascade_fixture())
            .output()
            .unwrap()
            .stdout
    };
    let first = run();
    let second = run();
    assert!(!first.is_empty(), "expected non-empty --json output");
    assert_eq!(
        first, second,
        "verify-el --json must be byte-identical across separate process runs \
         on the same input — see issue #59"
    );
}

/// Same determinism requirement, on the plain-text (non-`--json`) output
/// mode too — `print_verify_el_text` shares the same sorting helpers as the
/// `--json` builder, and this pins that they agree.
#[test]
fn text_output_is_byte_identical_across_two_separate_process_runs() {
    let run = || -> Vec<u8> {
        rustdl()
            .arg("verify-el")
            .arg(cascade_fixture())
            .output()
            .unwrap()
            .stdout
    };
    let first = run();
    let second = run();
    assert!(!first.is_empty(), "expected non-empty text output");
    assert_eq!(
        first, second,
        "verify-el's plain-text output must be byte-identical across separate \
         process runs on the same input"
    );
}

/// `--json` on the `Violated` fixture: schema shape + exit code together,
/// and that `unresolved`/`violations` are present as arrays (not e.g.
/// silently omitted when empty, which would make an automated consumer's
/// life harder).
#[test]
fn json_violated_has_the_expected_shape() {
    let out = rustdl()
        .arg("verify-el")
        .arg("--json")
        .arg(violated_fixture())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).expect("valid JSON");
    assert_eq!(v["schema_version"], 1);
    assert_eq!(v["verdict"], "violated");
    assert!(v["violations"].is_array());
    assert!(!v["violations"].as_array().unwrap().is_empty());
    assert!(v["unresolved"].is_array());
    assert!(v["dropped"].is_object());
}

/// Exercises `fold_build_reasons`'s `Verified -> Unresolved` downgrade arm
/// through the ACTUAL BINARY, end to end: `topwitness.ofn`'s check verdict is
/// `Verified` (every written axiom holds), but `build_model` admits, in its
/// own `reasons`, that it could not close the ⊤-witness's label. Without the downgrade, the CLI would report exit 0 here — a false
/// all-clear over an admitted gap. With it, this must be exit 3, and the
/// build reason must be visible on both the text and `--json` surfaces, not
/// swallowed.
#[test]
fn build_reasons_downgrade_a_verified_check_to_unresolved_and_exit_three() {
    let out = rustdl()
        .arg("verify-el")
        .arg(residue_fixture())
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(3),
        "a Verified check paired with non-empty build_model reasons must be \
         downgraded to Unresolved (exit 3), never reported as exit 0 \
         (Verified) or exit 2 (Violated) — stdout: {}, stderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.starts_with("unresolved:"),
        "expected an unresolved: line, got {stdout:?}"
    );
    assert!(
        stdout.contains("LabelNotClosed"),
        "the build-time reason must be surfaced on the text path, not \
         swallowed by the downgrade: {stdout:?}"
    );

    let json_out = rustdl()
        .arg("verify-el")
        .arg("--json")
        .arg(residue_fixture())
        .output()
        .unwrap();
    assert_eq!(json_out.status.code(), Some(3));
    let v: serde_json::Value = serde_json::from_slice(&json_out.stdout).expect("valid JSON");
    assert_eq!(v["verdict"], "unresolved");
    assert_eq!(v["axioms_checked"], 0);
    let unresolved = v["unresolved"].as_array().expect("unresolved array");
    assert!(
        !unresolved.is_empty(),
        "the build reason must be surfaced on the --json path too: {v:?}"
    );
    assert!(
        unresolved
            .iter()
            .any(|r| r.as_str().unwrap_or("").contains("LabelNotClosed")),
        "expected a LabelNotClosed entry in unresolved: {unresolved:?}"
    );
}
