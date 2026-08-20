//! SOUNDNESS regression: a report position must never be confused with an
//! `owl_dl_core::ClassId`.
//!
//! `classify()` reports only *user* classes — the synthetic `DKey(range)`
//! filler classes minted by the integer-facet data lowering are excluded.
//! That exclusion used to be a `.filter()` applied AFTER enumerating
//! `0..num_classes()`, so report position `i` stopped being `ClassId::new(i)`
//! as soon as a `DKey` id was dropped from below it, and every reported class
//! above that `DKey` read its subsumption row off a NEIGHBOUR — producing
//! false positives in the public API.
//!
//! Why the curated corpus never caught it: `convert_ontology` sorts components
//! before lowering, and every `DeclareClass` sorts before every axiom, so in a
//! fully-`Declaration`-ed ontology all named classes are interned before any
//! axiom can mint a `DKey`. `bench-corpus/mie.ofn` declares all 84 of its
//! classes, so its 17 `DKey`s land at ids 84..=100 — above everything, where
//! the aliasing is invisible. The fixtures below instead reference classes
//! that are NOT declared, so they are interned *after* a `DKey`.
//!
//! Run: `cargo test -p owl-dl-reasoner --test dkey_id_aliasing`.

#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use std::io::Cursor;
use std::time::Duration;

const PFX: &str = r"Prefix(:=<http://t/>)
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)
";

fn parse(body: &str) -> SetOntology<RcStr> {
    let src = format!("{PFX}Ontology(<http://t/x>\n{body}\n)\n");
    let mut reader = Cursor::new(src);
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut reader, ParserConfiguration::default()).expect("parse ofn");
    onto
}

/// A `DKey` is minted while lowering the FIRST axiom (its `sup` is the
/// earliest-sorting `ClassExpression::Class`, and `SubClassOf` derives `Ord`
/// over `(sup, sub)`), so it takes class id 0. `Zzz`/`Yy`/`Uu` are undeclared
/// and get ids 1.. — every one of them above the `DKey`.
///
/// Ground truth: `Uu ⊑ Zzz ⊑ Yy` and nothing else.
const EL_BODY: &str = r"
SubClassOf(ObjectSomeValuesFrom(:op DataSomeValuesFrom(:dp xsd:integer)) :Aaa)
SubClassOf(:Zzz :Yy)
SubClassOf(:Uu :Zzz)
";

/// Same shape, but a disjointness axiom drags the input out of the EL /
/// saturator-complete fragment so the tier walk + tableau path runs instead of
/// the pure-EL fast path. Keeps the `DKey`-below-user-classes hazard.
const HYBRID_BODY: &str = r"
SubClassOf(ObjectSomeValuesFrom(:op DataSomeValuesFrom(:dp xsd:integer)) :Aaa)
SubClassOf(:Zzz ObjectUnionOf(:Yy :Ww))
SubClassOf(:Uu :Zzz)
SubClassOf(:Yy :Ww)
DisjointClasses(:Uu :Ppp)
";

/// The whole point of the fixtures: they must actually place a `DKey` BELOW at
/// least one reported class. If a future change to `convert_ontology`'s
/// component ordering pushes every `DKey` to the top of the id space again,
/// the oracles below would still pass — but they would be VACUOUS. This test
/// is the non-vacuity guard, and it is what to look at first if the oracles
/// ever go green for a suspicious reason.
#[test]
fn fixtures_really_put_a_dkey_below_a_user_class() {
    for (name, body) in [("EL", EL_BODY), ("hybrid", HYBRID_BODY)] {
        let onto = parse(body);
        let internal = owl_dl_core::convert::convert_ontology(&onto).expect("convert");
        let n = internal.vocabulary.num_classes();
        let iris: Vec<String> = (0..n)
            .map(|i| {
                internal
                    .vocabulary
                    .class_iri(owl_dl_core::ClassId::new(u32::try_from(i).unwrap()))
                    .to_string()
            })
            .collect();
        let is_dkey = |s: &str| s.starts_with(owl_dl_core::DKEY_IRI_PREFIX);
        let last_dkey = iris.iter().rposition(|s| is_dkey(s));
        let last_user = iris.iter().rposition(|s| !is_dkey(s));
        assert!(
            last_dkey.is_some(),
            "{name} fixture minted no DKey at all — the hazard is not exercised; \
             iris = {iris:#?}"
        );
        assert!(
            last_dkey < last_user,
            "{name} fixture puts every DKey ABOVE every user class, so report \
             position == ClassId and the aliasing oracles below are VACUOUS. \
             iris = {iris:#?}"
        );
    }
}

/// Oracle 1 — `classify()` must agree with `is_subclass_of()`.
///
/// `is_subclass_of` resolves both IRIs to `ClassId`s itself and never goes
/// through the report projection, so a disagreement is the projection lying.
/// Both directions are checked: a `classify`-yes / direct-no is a FALSE
/// POSITIVE (unacceptable), a `classify`-no / direct-yes is a MISSED
/// subsumption.
fn assert_classify_agrees_with_direct_query(
    label: &str,
    onto: &SetOntology<RcStr>,
    cls: &owl_dl_reasoner::Classification,
    check_missed: bool,
) {
    // `check_missed == false` also means "only probe the pairs classify reported"
    // — on a real ontology the full n² of fresh `is_subclass_of` calls is
    // minutes of tableau work, and the FP direction only needs the positives.
    let names: Vec<String> = cls.classes().iter().map(|s| (*s).clone()).collect();
    let mut fps: Vec<(String, String)> = Vec::new();
    let mut missed: Vec<(String, String)> = Vec::new();
    for sub in &names {
        for sup in &names {
            if sub == sup {
                continue;
            }
            let reported = cls.is_subclass(sub, sup);
            if !reported && !check_missed {
                continue;
            }
            let direct = owl_dl_reasoner::is_subclass_of(onto, sub, sup).expect("is_subclass_of");
            if reported && !direct {
                fps.push((sub.clone(), sup.clone()));
            } else if direct && !reported {
                missed.push((sub.clone(), sup.clone()));
            }
        }
    }
    assert!(
        fps.is_empty(),
        "[{label}] classify() reported subsumptions that is_subclass_of() denies \
         — FALSE POSITIVES: {fps:#?}"
    );
    if check_missed {
        assert!(
            missed.is_empty(),
            "[{label}] classify() lost subsumptions is_subclass_of() confirms \
             — MISSED: {missed:#?}"
        );
    }
}

#[test]
fn classify_agrees_with_direct_query_on_el_fixture() {
    let onto = parse(EL_BODY);
    let cls = owl_dl_reasoner::classify(&onto).expect("classify");
    assert_classify_agrees_with_direct_query("classify/EL", &onto, &cls, true);
}

#[test]
fn classify_agrees_with_direct_query_on_hybrid_fixture() {
    let onto = parse(HYBRID_BODY);
    let cls = owl_dl_reasoner::classify(&onto).expect("classify");
    assert_classify_agrees_with_direct_query("classify/hybrid", &onto, &cls, true);
}

/// The naive `n²` pair sweep has its own copy of the report-index → `ClassId`
/// mapping, so it needs its own cross-check.
#[test]
fn classify_n2_agrees_with_direct_query() {
    for (label, body) in [("n2/EL", EL_BODY), ("n2/hybrid", HYBRID_BODY)] {
        let onto = parse(body);
        let cls = owl_dl_reasoner::classify_n2(&onto).expect("classify_n2");
        assert_classify_agrees_with_direct_query(label, &onto, &cls, true);
    }
}

/// The saturation-only path is a documented sound UNDER-approximation, so only
/// the false-positive direction is asserted.
#[test]
fn classify_saturation_only_has_no_false_positives() {
    for (label, body) in [("sat/EL", EL_BODY), ("sat/hybrid", HYBRID_BODY)] {
        let onto = parse(body);
        let cls =
            owl_dl_reasoner::classify_saturation_only(&onto).expect("classify_saturation_only");
        assert_classify_agrees_with_direct_query(label, &onto, &cls, false);
    }
}

#[test]
fn classify_top_down_agrees_with_direct_query() {
    for (label, body) in [("td/EL", EL_BODY), ("td/hybrid", HYBRID_BODY)] {
        let onto = parse(body);
        let cls = owl_dl_reasoner::classify_top_down_with_timeout(&onto, Duration::from_secs(5))
            .expect("classify_top_down");
        assert_classify_agrees_with_direct_query(label, &onto, &cls, true);
    }
}

/// Oracle 2 — the INERT-DECLARATION property, and the one that generalises.
///
/// `Declaration(Class(:C))` for a `C` the ontology already mentions entails
/// NOTHING: OWL 2 treats a used-but-undeclared class as declared, so the two
/// ontologies have identical models. A correct classifier must therefore
/// report the identical hierarchy. Adding the declarations *does* change
/// `ClassId` assignment though — `DeclareClass` sorts before every axiom, so
/// the declared classes are interned first and every `DKey` moves above them,
/// which is exactly the configuration where the aliasing is invisible. So
/// under the bug the declared variant is CORRECT and the undeclared one is
/// BROKEN, and the two hierarchies differ.
fn assert_inert_declarations_are_inert(label: &str, body: &str, declarations: &str) {
    let bare = parse(body);
    let declared = parse(&format!("{declarations}{body}"));

    let bare_cls = owl_dl_reasoner::classify(&bare).expect("classify bare");
    let declared_cls = owl_dl_reasoner::classify(&declared).expect("classify declared");

    let mut bare_names: Vec<String> = bare_cls.classes().iter().map(|s| (*s).clone()).collect();
    let mut declared_names: Vec<String> = declared_cls
        .classes()
        .iter()
        .map(|s| (*s).clone())
        .collect();
    bare_names.sort();
    declared_names.sort();
    assert_eq!(
        bare_names, declared_names,
        "[{label}] inert declarations changed the reported CLASS SET"
    );

    let mut differences: Vec<String> = Vec::new();
    for sub in &bare_names {
        for sup in &bare_names {
            let before = bare_cls.is_subclass(sub, sup);
            let after = declared_cls.is_subclass(sub, sup);
            if before != after {
                differences.push(format!(
                    "{sub} <= {sup}: without declarations = {before}, with = {after}"
                ));
            }
        }
    }
    assert!(
        differences.is_empty(),
        "[{label}] adding Declaration(Class(..)) axioms that entail NOTHING changed \
         the reported hierarchy — impossible for a correct classifier:\n{}",
        differences.join("\n")
    );
}

#[test]
fn inert_declarations_do_not_change_the_hierarchy_el() {
    assert_inert_declarations_are_inert(
        "EL",
        EL_BODY,
        "Declaration(Class(:Aaa))\n\
         Declaration(Class(:Zzz))\n\
         Declaration(Class(:Yy))\n\
         Declaration(Class(:Uu))\n",
    );
}

#[test]
fn inert_declarations_do_not_change_the_hierarchy_hybrid() {
    assert_inert_declarations_are_inert(
        "hybrid",
        HYBRID_BODY,
        "Declaration(Class(:Aaa))\n\
         Declaration(Class(:Zzz))\n\
         Declaration(Class(:Yy))\n\
         Declaration(Class(:Uu))\n\
         Declaration(Class(:Ww))\n\
         Declaration(Class(:Ppp))\n",
    );
}

/// General soundness cross-check on a tracked corpus fixture. `mie.ofn` does
/// NOT currently exercise the aliasing (all 84 classes are declared, so its
/// `DKey`s sit at ids 84..=100) — see the module doc. It is here as a
/// real-ontology FP oracle, not as a reproducer.
#[test]
fn classify_agrees_with_direct_query_on_mie_corpus() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../bench-corpus/mie.ofn");
    let src = std::fs::read_to_string(&path).expect("read mie.ofn");
    let mut reader = Cursor::new(src);
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut reader, ParserConfiguration::default()).expect("parse mie.ofn");
    let cls = owl_dl_reasoner::classify(&onto).expect("classify");
    assert!(
        cls.classes().len() > 50,
        "mie.ofn should report ~84 classes, got {}",
        cls.classes().len()
    );
    assert_classify_agrees_with_direct_query("mie", &onto, &cls, false);
}

/// Mechanical guard: the two raw spellings that re-arm this bug —
/// `ClassId::new(...)` over a report position, and `id.index() as usize` used
/// as one — must appear ONLY inside `classify.rs`'s declared conversion
/// boundary (the `ReportedClasses` struct + impl). Anywhere else in the
/// production body of that file they are the bug.
///
/// The file's own inline `mod tests` is excluded: its hand-built fixtures are
/// declaration-only, so report position and `ClassId` genuinely coincide there
/// and it indexes by `id.index()` deliberately.
#[test]
fn report_positions_are_never_cast_to_class_ids() {
    const BEGIN: &str = "BEGIN report-position ↔ ClassId conversion boundary";
    const END: &str = "END report-position ↔ ClassId conversion boundary";
    let src = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/classify.rs"));
    let production = src
        .split_once("\n#[cfg(test)]")
        .map_or(src, |(before, _)| before);

    let mut inside_boundary = false;
    let mut offenders: Vec<String> = Vec::new();
    for (lineno, line) in production.lines().enumerate() {
        if line.contains(BEGIN) {
            inside_boundary = true;
            continue;
        }
        if line.contains(END) {
            inside_boundary = false;
            continue;
        }
        if inside_boundary {
            continue;
        }
        let code = line.trim_start();
        if code.starts_with("//") {
            continue; // doc comment / prose
        }
        // Split so this test's own source never matches itself.
        let mint = concat!("ClassId::new", "(");
        let cast = concat!(".index() as", " usize");
        if code.contains(mint) || code.contains(cast) {
            offenders.push(format!("classify.rs:{}: {}", lineno + 1, line.trim_end()));
        }
    }
    assert!(
        offenders.is_empty(),
        "raw report-position ↔ ClassId conversion(s) outside the declared \
         boundary in classify.rs. A report position is NOT a ClassId (DKey \
         filler classes are removed from the report vector, so the two spaces \
         differ as soon as a DKey id sits below a user class — that is the \
         false-positive bug this file guards). Use \
         `ReportedClasses::class_id` / `ReportedClasses::report_pos` instead, \
         or move the code inside the boundary sentinels if it genuinely \
         operates in id space.\n{}",
        offenders.join("\n")
    );
}
