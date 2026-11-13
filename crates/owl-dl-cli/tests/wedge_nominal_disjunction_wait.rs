//! #480, `RUSTDL_WEDGE_NOMINAL_DISJUNCTION_WAIT`: the consistency wedge does not let
//! a node generate while it carries an open disjunction with a nominal disjunct.
//!
//! In generated case 939, `C1 ≡ ¬C3 ⊓ {a2, a0}` makes every node `a2` or `a0`, but
//! the wedge decides that disjunction only after its Horn fixpoint. Each new node
//! built its successors first, the search then merged it into a nominal and moved
//! them, and the moved successor did the same, until the deadline. `consistent`
//! waited out `RUSTDL_CONSISTENCY_FALLBACK_MS`, and `classify` reported
//! `consistency_undetermined`. With the wait, a deferred generating clause is
//! parked until the search picks a disjunct.
//!
//! The signal that the wedge itself decided is `relies_on_trusted_consistency`
//! (#392): it is set only when the verdict is the wedge's `Sat`. Every command's
//! content must be the same with the wait on and off. The inconsistent canary needs
//! the parked `∃` clauses to fire, so a clause that stayed parked would show as a
//! wrong `consistent`.
#![allow(clippy::unwrap_used)]

use std::path::PathBuf;
use std::process::{Command, Output};

const PREFIX: &str = "Prefix(owl:=<http://www.w3.org/2002/07/owl#>)\n\
                      Prefix(:=<http://gen.rustdl/o#>)\n\
                      Ontology(<http://gen.rustdl/o>\n";

const NS: &str = "http://gen.rustdl/o#";

/// Generated case 939 (#473, #480), all 14 axioms. Consistent: Konclude and
/// `HermiT` agree (the #363 oracle record for this file).
const CASE_939: &str = "Declaration(Class(:C0))
Declaration(Class(:C1))
Declaration(Class(:C2))
Declaration(Class(:C3))
Declaration(Class(:C4))
Declaration(ObjectProperty(:r0))
Declaration(ObjectProperty(:r1))
Declaration(DataProperty(:d0))
Declaration(DataProperty(:d1))
Declaration(NamedIndividual(:a0))
Declaration(NamedIndividual(:a1))
Declaration(NamedIndividual(:a2))
TransitiveObjectProperty(:r1)
EquivalentClasses(:C0 ObjectSomeValuesFrom(:r1 ObjectSomeValuesFrom(:r0 :C0)))
SubClassOf(ObjectComplementOf(:C0) :C4)
SubClassOf(:C3 ObjectSomeValuesFrom(:r1 ObjectOneOf(:a2)))
SubClassOf(ObjectSomeValuesFrom(:r0 ObjectOneOf(:a2)) :C0)
DataPropertyDomain(:d0 :C1)
EquivalentClasses(:C1 ObjectIntersectionOf(ObjectComplementOf(:C3) ObjectOneOf(:a2 :a0)))
EquivalentClasses(:C0 :C4)
EquivalentClasses(:C4 :C1)
EquivalentClasses(:C4 ObjectSomeValuesFrom(:r1 :C4))
SubClassOf(:C0 :C4)
SubClassOf(:C2 ObjectSomeValuesFrom(:r0 ObjectUnionOf(:C1 :C0)))
SubClassOf(:C3 ObjectUnionOf(:C4 ObjectExactCardinality(0 :r0 :C2)))
InverseFunctionalObjectProperty(:r0)";

/// Case 939 minimised (ddmin on "the wedge stalls without the wait and decides
/// with it"): every element is `C0 ≡ C4 ≡ C1`, so `a2` or `a0`, and has an
/// `r1`-successor with an `r0`-successor. Consistent, and `C3` is unsatisfiable.
const MIN_939: &str =
    "EquivalentClasses(:C0 ObjectSomeValuesFrom(:r1 ObjectSomeValuesFrom(:r0 :C0)))
SubClassOf(ObjectComplementOf(:C0) :C4)
EquivalentClasses(:C1 ObjectIntersectionOf(ObjectComplementOf(:C3) ObjectOneOf(:a2 :a0)))
EquivalentClasses(:C0 :C4)
EquivalentClasses(:C4 :C1)";

/// Inconsistent: `MIN_939` plus "an `r0`-successor makes a node `¬C1`". Every node
/// is `C1`, and the `r1`-successor needs an `r0`-successor. Only fired (parked,
/// then released) `∃` clauses find it.
const INC_PARKED: &str =
    "EquivalentClasses(:C0 ObjectSomeValuesFrom(:r1 ObjectSomeValuesFrom(:r0 :C0)))
SubClassOf(ObjectComplementOf(:C0) :C4)
EquivalentClasses(:C1 ObjectIntersectionOf(ObjectComplementOf(:C3) ObjectOneOf(:a2 :a0)))
EquivalentClasses(:C0 :C4)
EquivalentClasses(:C4 :C1)
SubClassOf(ObjectSomeValuesFrom(:r0 owl:Thing) :D)
SubClassOf(:D ObjectComplementOf(:C1))";

/// Writes a fixture to a per-test file (tests run in parallel).
struct Fixture(PathBuf);

impl Fixture {
    fn new(tag: &str, axioms: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "rustdl-wedge-nominal-disjunction-wait-{}-{tag}.ofn",
            std::process::id()
        ));
        std::fs::write(&path, format!("{PREFIX}{axioms}\n)\n")).unwrap();
        Self(path)
    }

    fn path(&self) -> &str {
        self.0.to_str().unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// The wedge budget. The wait decides these inputs in milliseconds; without it the
/// wedge never settles, so a short budget keeps the OFF arm quick.
const BUDGET_MS: &str = "2000";

/// `rustdl <args>` with no inherited `RUSTDL_*` settings, the wait on or off.
fn rustdl(args: &[&str], wait: bool) -> Output {
    rustdl_with(args, Some(wait))
}

/// As [`rustdl`]; `None` leaves the wait at its default.
fn rustdl_with(args: &[&str], wait: Option<bool>) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_rustdl"));
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("RUSTDL_") {
            cmd.env_remove(key);
        }
    }
    cmd.args(args)
        .env("RUSTDL_CONSISTENCY_FALLBACK_MS", BUDGET_MS);
    if let Some(wait) = wait {
        cmd.env(
            "RUSTDL_WEDGE_NOMINAL_DISJUNCTION_WAIT",
            if wait { "1" } else { "0" },
        );
    }
    cmd.output().unwrap()
}

fn json(out: &Output, what: &str) -> serde_json::Value {
    let stdout = String::from_utf8_lossy(&out.stdout);
    serde_json::from_str(&stdout).unwrap_or_else(|e| {
        panic!(
            "{what}: not JSON ({e}): {stdout:?}\nstderr: {}",
            String::from_utf8_lossy(&out.stderr)
        )
    })
}

/// The answer part of a `classify --json` / `realize --json` object, as the
/// measurement rule compares it: `consistent`, unsatisfiable classes, equivalence
/// groups and rows, and realize's per-individual types. Timing and trust signals,
/// which the wait is meant to change, and diagnostic counters are left out.
fn content(v: &serde_json::Value) -> serde_json::Value {
    let mut out = serde_json::Map::new();
    for key in [
        "consistent",
        "unsatisfiable",
        "equivalent_groups",
        "direct_subsumptions",
        "individuals",
    ] {
        if let Some(x) = v.get(key) {
            out.insert(key.to_owned(), x.clone());
        }
    }
    assert!(!out.is_empty(), "no content fields in {v}");
    serde_json::Value::Object(out)
}

fn wedge_decides(tag: &str, axioms: &str) {
    let f = Fixture::new(tag, axioms);
    let on = rustdl(&["consistent", "--json", f.path()], true);
    let on = json(&on, tag);
    assert_eq!(on["consistent"], true, "[{tag}]");
    assert_eq!(
        on["relies_on_trusted_consistency"], true,
        "[{tag}] the consistency wedge must settle and decide this itself (#480); \
         `false` means it was cut and the main tableau answered"
    );
    let off = json(&rustdl(&["consistent", "--json", f.path()], false), tag);
    assert_eq!(
        off["consistent"], true,
        "[{tag}] the main tableau's verdict"
    );
    assert_eq!(
        off["relies_on_trusted_consistency"], false,
        "[{tag}] without the wait the wedge does not settle (if it now does, this test \
         no longer pins #480)"
    );
}

#[test]
fn the_wedge_decides_case_939() {
    wedge_decides("case939", CASE_939);
}

#[test]
fn the_wedge_decides_case_939_minimised() {
    wedge_decides("min939", MIN_939);
}

/// The wait is on by default: with no setting, the wedge decides case 939.
#[test]
fn the_wait_is_on_by_default() {
    let f = Fixture::new("default", MIN_939);
    let r = json(
        &rustdl_with(&["consistent", "--json", f.path()], None),
        "default",
    );
    assert_eq!(r["consistent"], true);
    assert_eq!(
        r["relies_on_trusted_consistency"], true,
        "the default must decide case 939 on the wedge (#480)"
    );
}

/// The parked `∃` clauses must fire before the wedge reports a model.
#[test]
fn a_parked_clause_still_finds_the_inconsistency() {
    let f = Fixture::new("inc", INC_PARKED);
    for wait in [true, false] {
        let r = json(&rustdl(&["consistent", "--json", f.path()], wait), "inc");
        assert_eq!(r["consistent"], false, "wait={wait}");
    }
}

/// Every command answers the same with the wait on and off.
///
/// `classify`'s `consistency_undetermined` is not asserted here: besides the wedge
/// it has timing-driven sources (a prep deadline, the consistency probe giving up),
/// which a loaded debug build can trip. The wedge's own verdict is pinned by the
/// `consistent` tests above.
#[test]
fn every_command_answers_the_same() {
    for (tag, axioms) in [("same939", CASE_939), ("samemin", MIN_939)] {
        let f = Fixture::new(tag, axioms);
        let c3 = format!("{NS}C3");
        let c1 = format!("{NS}C1");
        let c0 = format!("{NS}C0");
        let classify = |wait| json(&rustdl(&["classify", "--json", f.path()], wait), tag);
        let (on, off) = (classify(true), classify(false));
        assert!(
            on["unsatisfiable"]
                .as_array()
                .unwrap()
                .iter()
                .any(|c| c == c3.as_str()),
            "[{tag}] C3 is unsatisfiable: {}",
            on["unsatisfiable"]
        );
        assert_eq!(content(&on), content(&off), "[{tag}] classify");

        let realize = |wait| json(&rustdl(&["realize", "--json", f.path()], wait), tag);
        let (on, off) = (realize(true), realize(false));
        // #430 coupling: with the wait, the wedge's `Sat` model becomes realize's
        // witness, so the prune may activate. Printed for the record.
        eprintln!(
            "[{tag}] realize witness_prune_reasons: on {} / off {}",
            on["witness_prune_reasons"], off["witness_prune_reasons"]
        );
        assert_eq!(content(&on), content(&off), "[{tag}] realize");

        for args in [
            vec!["sat", f.path(), c3.as_str()],
            vec!["sat", f.path(), c1.as_str()],
            vec!["subclass", f.path(), c1.as_str(), c0.as_str()],
            vec!["subclass", f.path(), c3.as_str(), c1.as_str()],
        ] {
            let (on, off) = (rustdl(&args, true), rustdl(&args, false));
            assert_eq!(on.status.code(), off.status.code(), "[{tag}] {args:?}");
            assert_eq!(
                String::from_utf8_lossy(&on.stdout),
                String::from_utf8_lossy(&off.stdout),
                "[{tag}] {args:?}"
            );
        }
    }
}
