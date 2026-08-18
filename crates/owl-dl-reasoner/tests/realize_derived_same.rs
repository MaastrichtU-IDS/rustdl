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
