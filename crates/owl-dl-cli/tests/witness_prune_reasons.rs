//! #387: `realize` reports when its pseudo-model prune may have dropped a type.
//!
//! The prune answers "not a type" from one witness model instead of a probe. The
//! witness can lack an entailed type when its search chose a disjunct, or when the
//! engine that built it did not get the whole KB, so `realize --json` carries
//! `witness_prune_may_drop` and the reasons, and text mode prints one stderr line.
//! Each env toggle here is read per call, so each case runs its own process.
//!
//! `false` is "no known reason for loss", not a guarantee; these tests pin that
//! each reason sets the flag and that a branch-free, fully clausified witness does
//! not, not that a `false` witness is complete.

#![allow(clippy::unwrap_used)]

use std::process::Command;

fn fixture(name: &str) -> String {
    format!(
        "{}/tests/fixtures/witness_prune/{name}",
        env!("CARGO_MANIFEST_DIR")
    )
}

/// `realize --json` with `env`; the search shortcuts that could settle a
/// disjunction without a decision are pinned off, so `branched` is the engine's
/// own choice and not an artefact of their defaults.
fn realize_json(name: &str, env: &[(&str, &str)]) -> serde_json::Value {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_rustdl"));
    cmd.env("RUSTDL_SEMANTIC_BRANCHING", "0")
        .env("RUSTDL_SAT_LOOKAHEAD", "0");
    for (k, v) in env {
        cmd.env(k, v);
    }
    let out = cmd
        .args(["realize", "--json", &fixture(name)])
        .output()
        .unwrap();
    assert!(out.status.success(), "{name}: {out:?}");
    serde_json::from_slice(&out.stdout).expect("stdout is valid JSON")
}

fn reasons(v: &serde_json::Value) -> Vec<&str> {
    v["witness_prune_reasons"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r.as_str().unwrap())
        .collect()
}

fn individuals(v: &serde_json::Value) -> &serde_json::Value {
    &v["individuals"]
}

/// A Horn `ABox`: the witness is the deterministic closure, so the prune is active
/// with no reason to drop anything.
#[test]
fn a_horn_abox_reads_false_with_the_prune_active() {
    let v = realize_json("horn.ofn", &[]);
    assert_eq!(v["witness_prune_active"], true);
    assert_eq!(v["witness_prune_may_drop"], false);
    assert!(reasons(&v).is_empty(), "{v}");
    assert_eq!(v["incomplete"], false);
}

/// `a : D` with `D ⊑ F ⊔ G`: the witness search must choose a disjunct.
#[test]
fn a_disjunct_choice_reads_branched() {
    let v = realize_json("union.ofn", &[]);
    assert_eq!(v["witness_prune_active"], true);
    assert_eq!(v["witness_prune_may_drop"], true);
    assert_eq!(reasons(&v), ["branched"]);
    // Only a signal: no probe was cut, and the types are those of the unpruned run.
    assert_eq!(v["incomplete"], false);
    let off = realize_json("union.ofn", &[("RUSTDL_PSEUDO_MODEL", "0")]);
    assert_eq!(individuals(&v), individuals(&off));
}

/// With the prune off there is no witness, so nothing can be dropped by it.
#[test]
fn with_the_prune_off_both_read_false() {
    let v = realize_json("union.ofn", &[("RUSTDL_PSEUDO_MODEL", "0")]);
    assert_eq!(v["witness_prune_active"], false);
    assert_eq!(v["witness_prune_may_drop"], false);
    assert!(reasons(&v).is_empty(), "{v}");
}

/// `DisjointClasses(∀r.F, G)`: the clausifier defers the `∀` antecedent, so the
/// witness engine never sees that axiom.
#[test]
fn a_deferred_axiom_reads_deferred_clauses() {
    let v = realize_json("deferred.ofn", &[]);
    assert_eq!(v["witness_prune_may_drop"], true);
    assert_eq!(reasons(&v), ["deferred_clauses"]);
}

/// The 12-variable body of `max_body_vars/wide-body-12vars.ofn` plus an
/// individual: with body decomposition off the matcher refuses that clause.
#[test]
fn a_refused_clause_reads_refused_clauses() {
    let v = realize_json("wide_body.ofn", &[("RUSTDL_WEDGE_BODY_DECOMPOSE", "0")]);
    assert_eq!(v["witness_prune_may_drop"], true);
    assert_eq!(reasons(&v), ["refused_clauses"]);
}

/// The two flags whose `=0` makes the witness miss work with no choice involved.
#[test]
fn the_reseed_net_and_inverse_functional_max_flags_read_their_own_reasons() {
    let v = realize_json("horn.ofn", &[("RUSTDL_RESEED_VERIFY", "0")]);
    assert_eq!(reasons(&v), ["reseed_net_off"]);
    assert_eq!(v["witness_prune_may_drop"], true);
    let v = realize_json("horn.ofn", &[("RUSTDL_INVERSE_FUNC_MAX", "0")]);
    assert_eq!(reasons(&v), ["inverse_functional_max_off"]);
    assert_eq!(v["witness_prune_may_drop"], true);
}

/// Text mode prints one stderr line naming the reasons when the flag is set, and
/// none when only the bare `witness_prune_active` holds.
#[test]
fn text_mode_warns_only_when_the_prune_may_drop() {
    let stderr = |name: &str| {
        let out = Command::new(env!("CARGO_BIN_EXE_rustdl"))
            .env("RUSTDL_SEMANTIC_BRANCHING", "0")
            .env("RUSTDL_SAT_LOOKAHEAD", "0")
            .args(["realize", &fixture(name)])
            .output()
            .unwrap();
        assert!(out.status.success(), "{name}: {out:?}");
        String::from_utf8(out.stderr).unwrap()
    };
    let union = stderr("union.ofn");
    let line = union
        .lines()
        .find(|l| l.contains("pseudo-model prune"))
        .expect("a warning line on a branched witness");
    assert!(line.contains("(branched)"), "{line}");
    assert!(
        !stderr("horn.ofn").contains("pseudo-model prune"),
        "no warning on a branch-free witness"
    );
}
