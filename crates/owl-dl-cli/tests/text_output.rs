//! PR #50 review Fix 6 (Minor): plain-text (non-`--json`) golden tests for
//! `disjoint`, `property-hierarchy`, `individuals`, `property-values` —
//! presentation-only coverage of the human-readable CLI output, over the
//! same tiny fixtures `tests/json_output.rs` already uses for `--json`.
//! `--json` mode is the stable, tested tooling contract (`docs/json-schema.md`);
//! this file exists so the OTHER (default, human) output mode isn't
//! completely untested.

#![allow(clippy::unwrap_used)]

use std::process::Command;

fn rustdl() -> Command {
    Command::new(env!("CARGO_BIN_EXE_rustdl"))
}

fn disjoint_tiny() -> &'static str {
    concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/json/disjoint_tiny.ofn"
    )
}

fn prophier_tiny() -> &'static str {
    concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/json/prophier_tiny.ofn"
    )
}

fn individuals_tiny() -> &'static str {
    concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/json/individuals_tiny.ofn"
    )
}

fn propvalues_tiny() -> &'static str {
    concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/json/propvalues_tiny.ofn"
    )
}

/// `disjoint` (no `--json`): section headers + tab-separated pairs on
/// stdout. `:A`/`:B` are told-disjoint; `:C ⊑ :A` makes `:B`/`:C` an
/// ENTAILED (non-told) disjoint pair too (`C ⊓ B ⊆ A ⊓ B = ∅`) — exercising
/// the tableau probe, not just the told-disjoint short-circuit. No object/
/// data property axioms in this fixture ⟹ both structural sections are
/// empty, and the result is complete (no stderr warning).
#[test]
fn disjoint_text_output() {
    let out = rustdl()
        .args(["disjoint", disjoint_tiny()])
        .output()
        .unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert_eq!(
        stdout,
        "# disjoint classes\n\
         http://ex/#A\thttp://ex/#B\n\
         http://ex/#B\thttp://ex/#C\n\
         # disjoint object properties\n\
         # disjoint data properties\n"
    );
    assert!(
        out.stderr.is_empty(),
        "expected no incomplete warning, got {:?}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// `property-hierarchy` (no `--json`): section headers + tab-separated
/// direct-subsumption edges for object then data properties. This query is
/// always complete-by-construction, so there is never an `incomplete`
/// stderr warning for it (see `docs/json-schema.md`).
#[test]
fn property_hierarchy_text_output() {
    let out = rustdl()
        .args(["property-hierarchy", prophier_tiny()])
        .output()
        .unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert_eq!(
        stdout,
        "# object property hierarchy\n\
         http://ex/#r\thttp://ex/#s\n\
         # data property hierarchy\n\
         http://ex/#d1\thttp://ex/#d2\n"
    );
    assert!(out.stderr.is_empty());
}

/// `individuals` (no `--json`): section headers + groups/pairs, followed by
/// the `incomplete` stderr warning. `:a`/`:b` are typed into told-disjoint
/// classes, so `different_individuals` proves `a≠b` via a genuine (not
/// told-different) `{a}⊓{b}` probe — and `same_individuals`'s honesty
/// policy flags `incomplete` whenever ANY extension probe beyond its sound
/// seed is consulted at all (even one that adds nothing), which is exactly
/// what happens here (no `SameIndividual` seed at all) — hence the warning.
#[test]
fn individuals_text_output() {
    let out = rustdl()
        .args(["individuals", individuals_tiny()])
        .output()
        .unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert_eq!(
        stdout,
        "# same individuals\n\
         # different individuals\n\
         http://ex/#a\thttp://ex/#b\n"
    );
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert_eq!(
        stderr,
        "warning: individuals incomplete (budget/fragment) — sound under-approximation\n"
    );
}

/// `property-values` (no `--json`): section headers + tab-separated triples/
/// quads. `:r` is symmetric and `r(a,b)` is asserted, so the seed alone
/// (`materialize_object_property_assertions`, which already closes symmetric
/// roles) surfaces both `r(a,b)` and the derived `r(b,a)` — no extension
/// candidates remain unresolved by the seed, so this fixture is complete
/// (no stderr warning).
#[test]
fn property_values_text_output() {
    let out = rustdl()
        .args(["property-values", propvalues_tiny()])
        .output()
        .unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert_eq!(
        stdout,
        "# object property values\n\
         http://ex/#a\thttp://ex/#r\thttp://ex/#b\n\
         http://ex/#b\thttp://ex/#r\thttp://ex/#a\n\
         # data property values\n\
         http://ex/#a\thttp://ex/#dp\t5\thttp://www.w3.org/2001/XMLSchema#integer\n"
    );
    assert!(
        out.stderr.is_empty(),
        "expected no incomplete warning, got {:?}",
        String::from_utf8_lossy(&out.stderr)
    );
}
