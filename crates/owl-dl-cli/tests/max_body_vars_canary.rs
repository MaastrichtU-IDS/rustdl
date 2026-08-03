//! Canaries for the `MAX_BODY_VARS` silent-MISS cap (`hyper.rs:46`) and the
//! `RUSTDL_WIDE_BODY_VARS=1` opt-in that raises it.
//!
//! **Why the cap is different from every other constant in `hyper.rs`.** The
//! budgets there (`FIXPOINT_ITERS`, `DIV_WINDOW`, `MAX_SEARCH_DEPTH`, …) turn
//! "ran out" into a *non-verdict*, so lowering them loses entailments and
//! raising them cannot invent one. `MAX_BODY_VARS` is not a budget: a body
//! over the cap makes `eval_order` refuse, `match_body` report `None`, and all
//! three consumers **skip the clause outright**. Its consequence is therefore a
//! **silent MISS** — an entailment quietly not derived, with no `incomplete`
//! signal — and raising it is the FP direction, because previously-dead clauses
//! start firing.
//!
//! **The fixture is what makes this measurable.** `wide-body-12vars.ofn`
//! clausifies `∃r1.A1 ⊓ … ⊓ ∃r11.A11 ⊑ B ⊔ C` into ONE non-Horn clause whose
//! body is a well-formed variable-tree rooted at `X` with **12 distinct
//! variables** (11 fresh successors + `X`) and 11 role atoms — the shape
//! `Clausifier::encode_antecedent`'s `ConceptExpr::Some` arm produces, one
//! fresh var per `∃` occurrence. Adding `DisjointClasses(X, B)` makes
//! `X ⊑ C` a genuine entailment that *requires* that clause to fire (the EL
//! saturator cannot do the case split, and there is no common told subsumer to
//! shortcut through).
//!
//! Confirmed by instrumentation, not by inspection: with
//! `RUSTDL_TRACE_BODY_VARS=1` the default build prints
//! `refused body: vars=12 role_atoms=11 cap=8 reason=VarCap { vars: 9, cap: 8 }`
//! — the `> MAX_BODY_VARS` branch specifically, not one of `eval_order`'s
//! other refusals (`NotTree` / `Disconnected`), which raising the cap could
//! never reach.
//!
//! `X ⊑ C` is adjudicated: **both Konclude v0.7.0 and `HermiT` 1.4.3 derive it**
//! (oracles committed beside the fixture as
//! `wide-body-12vars-konclude.owx` / `wide-body-12vars-hermit.txt`), so the
//! flag-ON arm recovers a real entailment rather than inventing one.
//!
//! The flag is read through a `OnceLock`, so the two arms cannot coexist in one
//! process; these tests therefore drive the built `rustdl` binary in a
//! subprocess, the same idiom as `incremental_fixpoint_identity.rs`.
#![allow(clippy::unwrap_used)]

use std::path::{Path, PathBuf};
use std::process::Command;

/// The entailment the wide-body clause is solely responsible for.
const WIDE_PAIR: &str = "direct\thttp://ex.org/mbv#X\thttp://ex.org/mbv#C";

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/max_body_vars")
        .join(name)
}

/// `classify` the fixture, returning `(verdict lines, stderr)`. `wide` selects
/// the flag arm; `None` leaves `RUSTDL_WIDE_BODY_VARS` unset so the shipped
/// default is exercised rather than an explicit `0`.
fn classify(ofn: &Path, wide: Option<bool>, trace: bool) -> (Vec<String>, String) {
    let mut c = Command::new(env!("CARGO_BIN_EXE_rustdl"));
    c.arg("classify").arg(ofn);
    match wide {
        Some(true) => {
            c.env("RUSTDL_WIDE_BODY_VARS", "1");
        }
        Some(false) => {
            c.env("RUSTDL_WIDE_BODY_VARS", "0");
        }
        None => {
            c.env_remove("RUSTDL_WIDE_BODY_VARS");
        }
    }
    if trace {
        c.env("RUSTDL_TRACE_BODY_VARS", "1");
    }
    let out = c.output().expect("run rustdl");
    let lines = String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|l| !l.starts_with('#'))
        .map(str::to_owned)
        .collect();
    (lines, String::from_utf8_lossy(&out.stderr).into_owned())
}

/// The cap is REACHED, and reached through the variable-count branch. This is
/// the instrument-fires check: a fixture that merely *looks* like it should
/// trip the cap proves nothing, and `eval_order` has three other ways to
/// refuse a body that raising the cap cannot address.
#[test]
fn wide_body_trips_the_variable_cap_specifically() {
    let (_, err) = classify(&fixture("wide-body-12vars.ofn"), None, true);
    let refused: Vec<&str> = err
        .lines()
        .filter(|l| l.starts_with("[mbv] refused body:"))
        .collect();
    assert!(
        !refused.is_empty(),
        "no body was refused at all — the fixture does not reach the cap.\nstderr:\n{err}"
    );
    assert!(
        refused.iter().any(|l| l.contains("reason=VarCap")),
        "a body was refused, but not by the variable cap (so raising the cap is \
         irrelevant to it).\nrefused lines: {refused:?}"
    );
    assert!(
        refused
            .iter()
            .any(|l| l.contains("vars=12") && l.contains("role_atoms=11")),
        "expected the 12-variable / 11-role-atom body; the clausifier shape changed \
         and the fixture no longer exercises what it claims.\nrefused lines: {refused:?}"
    );
}

/// The silent MISS itself, at the SHIPPED default. `X ⊑ C` is entailed
/// (Konclude and `HermiT` both derive it) and rustdl does not report it.
///
/// This assertion is deliberately phrased as "the default misses it" rather
/// than "rustdl is incomplete here": if the default is ever flipped, this test
/// fails and must be re-pointed at an explicit `RUSTDL_WIDE_BODY_VARS=0`,
/// which is the intended signal.
#[test]
fn shipped_default_silently_misses_the_wide_body_entailment() {
    let (lines, _) = classify(&fixture("wide-body-12vars.ofn"), None, false);
    assert!(
        !lines.iter().any(|l| l == WIDE_PAIR),
        "the default cap no longer misses X ⊑ C — if MAX_BODY_VARS or the default \
         flag state changed, re-point this test at RUSTDL_WIDE_BODY_VARS=0.\n\
         lines: {lines:?}"
    );
}

/// Raising the cap recovers it. Fails before the `RUSTDL_WIDE_BODY_VARS` lever
/// exists (the flag is ignored, so this arm equals the default arm above).
#[test]
fn wide_body_vars_recovers_the_entailment() {
    let (lines, err) = classify(&fixture("wide-body-12vars.ofn"), Some(true), true);
    assert!(
        err.lines()
            .any(|l| l.starts_with("[mbv] accepted body:") && l.contains("vars=12")),
        "the 12-variable body was not ACCEPTED under the raised cap, so any recovery \
         below would be for some other reason.\nstderr:\n{err}"
    );
    assert!(
        lines.iter().any(|l| l == WIDE_PAIR),
        "RUSTDL_WIDE_BODY_VARS=1 did not recover X ⊑ C, which Konclude and HermiT \
         both derive.\nlines: {lines:?}"
    );
}

/// Negative control on the OTHER side of the boundary: the same construction
/// at 8 variables (7 successors + `X`) is UNDER the shipped cap, so the
/// entailment is already found and the flag changes nothing. Without this, a
/// green suite could not distinguish "the cap is the cause" from "wide bodies
/// are broken for some unrelated reason".
#[test]
fn narrow_body_is_found_at_both_cap_settings() {
    let (off, _) = classify(&fixture("narrow-body-8vars.ofn"), None, false);
    let (on, _) = classify(&fixture("narrow-body-8vars.ofn"), Some(true), false);
    assert!(
        off.iter().any(|l| l == WIDE_PAIR),
        "the 8-variable control is NOT found at the default cap, so the fixture pair \
         does not isolate the cap.\nlines: {off:?}"
    );
    assert_eq!(
        off, on,
        "the flag changed the answer on a fixture that never reaches the cap"
    );
}

/// **The FP control, and the one that matters most.** Raising the cap makes
/// previously-dead clauses fire, which ADDS derived facts — the false-positive
/// direction, unlike every early-termination budget in `hyper.rs`. This fixture
/// is the same 12-variable clause `∃r1.A1 ⊓ … ⊓ ∃r11.A11 ⊑ B ⊔ C` with the
/// `DisjointClasses(X, B)` premise REMOVED, so `X ⊑ B ⊔ C` holds but neither
/// `X ⊑ B` nor `X ⊑ C` is entailed. The clause must fire (asserted, so a silent
/// non-fire cannot pass this test) and still yield no subsumption for `X`,
/// which is what the committed Konclude oracle reports too.
#[test]
fn wide_body_fires_without_inventing_a_subsumption() {
    let f = fixture("wide-body-12vars-no-entailment.ofn");
    let (lines, err) = classify(&f, Some(true), true);
    assert!(
        err.lines()
            .any(|l| l.starts_with("[mbv] accepted body:") && l.contains("vars=12")),
        "the wide body did not fire, so this test would pass vacuously.\nstderr:\n{err}"
    );
    let x_rows: Vec<&String> = lines
        .iter()
        .filter(|l| l.contains("http://ex.org/mbv#X"))
        .collect();
    assert!(
        x_rows.is_empty(),
        "firing the wide clause invented a subsumption for X that neither B nor C \
         entails — this is an FP.\nrows: {x_rows:?}"
    );
    // And the flag genuinely changed nothing here, per the same oracle.
    let (off, _) = classify(&f, None, false);
    assert_eq!(
        off, lines,
        "the flag changed a non-entailment fixture's answer"
    );
}

/// The oracles are committed, so the adjudication is reproducible without
/// Konclude or Docker on the machine running the suite.
#[test]
fn committed_oracles_both_derive_the_recovered_pair() {
    let kon = std::fs::read_to_string(fixture("wide-body-12vars-konclude.owx")).unwrap();
    let her = std::fs::read_to_string(fixture("wide-body-12vars-hermit.txt")).unwrap();
    // Konclude emits OWL/XML: the pair is an adjacent Class/Class pair inside
    // a SubClassOf element.
    let kon_flat: String = kon.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(
        kon_flat.contains(
            "<SubClassOf> <Class IRI=\"http://ex.org/mbv#X\"/> \
             <Class IRI=\"http://ex.org/mbv#C\"/> </SubClassOf>"
        ),
        "the committed Konclude oracle does not contain X ⊑ C"
    );
    assert!(
        her.contains("SubClassOf( <http://ex.org/mbv#X> <http://ex.org/mbv#C> )"),
        "the committed HermiT oracle does not contain X ⊑ C"
    );
}
