//! `realize` uses ASSERTED individual equality but ignores DERIVED equality.
//!
//! See `docs/known-limitations/realize-drops-derived-individual-equality.md`.
//!
//! `InverseFunctionalObjectProperty(:r)` with `r(x,z)` and `r(y,z)` entails `x = y`, so
//! `x` and `y` must share types. `rustdl individuals` DOES derive
//! `same_groups: [["x","y"]]`; `rustdl realize` reports `x : A` and `y : B` only, and
//! `realize --json` carries no `incomplete` field, so the miss is silent. The tableau
//! path (`RUSTDL_REALIZE_SATURATION=0`) misses it too, so that flag is not a
//! workaround.
//!
//! The asserted-equality control below documents that the machinery works when the
//! equality is written down — it passes today and must keep passing.

use owl_dl_reasoner::realize;
use std::sync::Mutex;

/// Serialises the env-mutating tests below. Cargo runs tests in one binary
/// CONCURRENTLY, and an earlier version of the sibling `realize_incomplete_signal.rs`
/// lost a debugging cycle to exactly this: one test set a budget var mid-realize in
/// another and the failure pointed at the wrong cause.
static ENV_MUTEX: Mutex<()> = Mutex::new(());
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

fn types_of(fixture: &str) -> BTreeMap<String, BTreeSet<String>> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/realize_derived_same")
        .join(fixture);
    let src = std::fs::read_to_string(&path).expect("read fixture");
    let (onto, _): (
        horned_owl::ontology::set::SetOntology<horned_owl::model::RcStr>,
        _,
    ) = horned_owl::io::ofn::reader::read(
        &mut std::io::Cursor::new(src),
        horned_owl::io::ParserConfiguration::default(),
    )
    .expect("parse fixture");
    let r = realize(&onto).expect("realize");
    let strip = |s: &str| s.replace("http://t/", "");
    let mut out: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for ind in r.individuals() {
        out.insert(
            strip(ind),
            r.entailed_types(ind).iter().map(|c| strip(c)).collect(),
        );
    }
    out
}

/// CONTROL — an explicit `SameIndividual(x,y)` is folded correctly. This is what makes
/// the ignored test below a genuine gap rather than an unimplemented feature: the
/// merge machinery exists and works, it just is not reached by a derived equality.
#[test]
fn asserted_equality_shares_types() {
    let t = types_of("asserted-same.ofn");
    let both: BTreeSet<String> = ["A", "B"].iter().map(|s| (*s).to_string()).collect();
    assert!(
        t.get("x").is_some_and(|s| both.is_subset(s)),
        "asserted SameIndividual(x,y) must give x both types, got {:?}",
        t.get("x")
    );
    assert!(
        t.get("y").is_some_and(|s| both.is_subset(s)),
        "asserted SameIndividual(x,y) must give y both types, got {:?}",
        t.get("y")
    );
}

/// KNOWN LIMITATION — asserts the CORRECT behaviour, currently fails.
///
/// `InverseFunctional(r) + r(x,z) + r(y,z) ⊨ x = y`, and `rustdl individuals` derives
/// exactly that, so `realize` must give both individuals both types. It does not.
/// Remove the `#[ignore]` when
/// `docs/known-limitations/realize-drops-derived-individual-equality.md` is closed.
///
/// A fix must cover BOTH realize paths: `RUSTDL_REALIZE_SATURATION=1` and `=0` are
/// equally wrong today, so folding `SaturationResult.derived_same` into the saturation
/// path alone would leave this failing under the flag.
#[test]
#[ignore = "known limitation: realize drops DERIVED individual equality (see docs/known-limitations/realize-drops-derived-individual-equality.md)"]
fn derived_equality_should_share_types() {
    let t = types_of("inverse-functional.ofn");
    let both: BTreeSet<String> = ["A", "B"].iter().map(|s| (*s).to_string()).collect();
    assert!(
        t.get("x").is_some_and(|s| both.is_subset(s)),
        "x = y is entailed by inverse-functionality, so x must have both types, got {:?}",
        t.get("x")
    );
    assert!(
        t.get("y").is_some_and(|s| both.is_subset(s)),
        "x = y is entailed by inverse-functionality, so y must have both types, got {:?}",
        t.get("y")
    );
}

/// FUNCTIONAL-forced equality on the DEFAULT path. **FIXED 2026-08-18** by option A of
/// `docs/superpowers/specs/2026-08-18-realize-derived-equality-design.md`:
/// `realize_saturation_eligible` now refuses an ontology carrying a functional /
/// inverse-functional role together with any `ObjectPropertyAssertion`, exactly as it
/// already refused `SameIndividual`, so the tableau — which DOES fold
/// functional-forced equality — realizes it.
///
/// Was `y : A` and `z : B`; now both individuals carry both types.
/// `Functional(r) + r(x,y) + r(x,z) ⊨ y = z`.
#[test]
fn derived_functional_equality_should_share_types() {
    let t = types_of("functional.ofn");
    let both: BTreeSet<String> = ["A", "B"].iter().map(|s| (*s).to_string()).collect();
    for i in ["y", "z"] {
        assert!(
            t.get(i).is_some_and(|s| both.is_subset(s)),
            "y = z is entailed by functionality, so {i} must have both types, got {:?}",
            t.get(i)
        );
    }
}

/// The ASYMMETRY between the two engines, pinned as a fact rather than left in prose:
/// the TABLEAU path handles functional-forced equality correctly. This is what makes
/// `RUSTDL_REALIZE_SATURATION=0` a usable workaround for the functional case and NOT
/// for the inverse-functional one.
///
/// Runs (not ignored) because it asserts behaviour that is correct today. If it ever
/// fails, the tableau has regressed and the functional workaround is gone.
#[test]
fn tableau_path_does_handle_functional_equality() {
    let _lock = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    // SAFETY: single-threaded within this test; no other test in this file reads the var.
    #[allow(unsafe_code)]
    unsafe {
        std::env::set_var("RUSTDL_REALIZE_SATURATION", "0");
    }
    let t = types_of("functional.ofn");
    #[allow(unsafe_code)]
    unsafe {
        std::env::remove_var("RUSTDL_REALIZE_SATURATION");
    }
    let both: BTreeSet<String> = ["A", "B"].iter().map(|s| (*s).to_string()).collect();
    for i in ["y", "z"] {
        assert!(
            t.get(i).is_some_and(|s| both.is_subset(s)),
            "the TABLEAU realize path does fold functional-forced equality today; {i} should \
             have both types, got {:?}. If this fails, the only workaround for the functional \
             case has regressed.",
            t.get(i)
        );
    }
}

/// ROOT CAUSE of the inverse-functional half: the **pseudo-model prune**, not the
/// tableau. `RUSTDL_PSEUDO_MODEL=0` returns the correct answer, and
/// `rustdl justify … instance x B` proves the membership with a 4-axiom minimal
/// justification — so the engine derives it and this prune discards it.
///
/// This **falsifies the prune's documented "sound by construction" argument**
/// (`realize.rs`: *"an entailed type is in every model, hence in the witness, hence
/// never pruned"*). That needs the witness to BE a model; on an inverse-functional
/// `ABox` it is not, because the witness build applies FUNCTIONAL merges but not
/// INVERSE-functional ones.
///
/// Subtractive, so FP=0 is intact — but the falsified clause is the stated basis for
/// shipping `RUSTDL_PSEUDO_MODEL` default-ON without the ORE bake-off, so that
/// bake-off is now load-bearing.
#[test]
fn pseudo_model_prunes_entailed_type() {
    let _lock = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    // SAFETY: guarded by ENV_MUTEX; restored below.
    #[allow(unsafe_code)]
    unsafe {
        std::env::set_var("RUSTDL_PSEUDO_MODEL", "0");
    }
    let with_prune_off = types_of("inverse-functional.ofn");
    #[allow(unsafe_code)]
    unsafe {
        std::env::remove_var("RUSTDL_PSEUDO_MODEL");
    }
    let both: BTreeSet<String> = ["A", "B"].iter().map(|s| (*s).to_string()).collect();
    for i in ["x", "y"] {
        assert!(
            with_prune_off.get(i).is_some_and(|s| both.is_subset(s)),
            "with the pseudo-model prune OFF the entailed merge must be reported for {i}, \
             got {:?}. If this fails the root cause has moved and the attribution in \
             `realize.rs` needs re-checking.",
            with_prune_off.get(i)
        );
    }
    // And the default really is worse — otherwise this test proves nothing.
    let with_prune_on = types_of("inverse-functional.ofn");
    assert!(
        !with_prune_on.get("x").is_some_and(|s| both.is_subset(s)),
        "the DEFAULT still prunes the entailed type; if it no longer does, the prune was \
         fixed and both this test and the falsification note in `realize.rs` should be \
         retired"
    );
}
