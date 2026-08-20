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
//! `bench-corpus/pizza.ofn` DOES have the hazardous layout (five user classes
//! above its first `DKey`), but every one of them is hierarchy-isolated, so it
//! shows no observable difference either way — see
//! `inert_declarations_do_not_change_the_hierarchy_pizza`. No tracked corpus
//! file can currently fail on this bug class; the hand-built fixtures are the
//! real net.
//!
//! Run: `cargo test -p owl-dl-reasoner --test dkey_id_aliasing`.

#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::{Build, DeclareClass, MutableOntology, RcStr};
use horned_owl::ontology::set::SetOntology;
use std::io::Cursor;
use std::time::Duration;

const PFX: &str = r"Prefix(:=<http://t/>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)
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

/// Same `DKey`-at-id-0 shape, plus an **unsatisfiable** class.
///
/// This is the fixture the first round of this file was missing, and the gap
/// let a live false positive survive the fix: `classify_pure_el`'s Pass 1 read
/// `Subsumers::unsatisfiable_bitset()` — which is `ClassId`-indexed ("bit `i`
/// set iff `class_i ⊑ ⊥`") — with a REPORT POSITION. A satisfiable class
/// inherited its neighbour's `⊥` flag, its row was elided, and
/// `Classification::entails` then supplied `⊥ ⊑ *` for it, so EVERY pair
/// involving it became a false positive and `unsatisfiable_classes()` named
/// the wrong class.
///
/// Ground truth: `Aaa ⊑ ⊥` (and only `Aaa`), `Ccc ⊑ Ddd ⊑ Eee`.
const UNSAT_BODY: &str = r"
SubClassOf(ObjectSomeValuesFrom(:op DataSomeValuesFrom(:dp xsd:integer)) :Aaa)
SubClassOf(:Aaa owl:Nothing)
SubClassOf(:Ccc :Ddd)
SubClassOf(:Ddd :Eee)
";

/// Add `Declaration(Class(C))` for every class the ontology already mentions.
///
/// Semantically inert (OWL 2 treats a used class as declared), but it moves
/// every named class BELOW every `DKey` in the id space — every `DeclareClass`
/// component sorts before every axiom — i.e. into the configuration where the
/// aliasing is invisible. Generated from the lowered vocabulary rather than
/// hand-listed so this works on a real corpus file too.
fn with_all_classes_declared(onto: &SetOntology<RcStr>) -> SetOntology<RcStr> {
    let internal = owl_dl_core::convert::convert_ontology(onto).expect("convert");
    let build: Build<RcStr> = Build::new();
    let mut out = onto.clone();
    for i in 0..internal.vocabulary.num_classes() {
        let iri = internal
            .vocabulary
            .class_iri(owl_dl_core::ClassId::new(u32::try_from(i).unwrap()));
        if iri.starts_with(owl_dl_core::DKEY_IRI_PREFIX) {
            continue;
        }
        out.insert(DeclareClass(build.class(iri)));
    }
    out
}

/// Load a tracked `bench-corpus/` fixture.
fn corpus(name: &str) -> SetOntology<RcStr> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../bench-corpus")
        .join(name);
    let src = std::fs::read_to_string(&path).expect("read corpus fixture");
    let mut reader = Cursor::new(src);
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut reader, ParserConfiguration::default()).expect("parse corpus fixture");
    onto
}

/// `true` iff some `DKey` id sits BELOW some user-class id — the configuration
/// in which a report position stops equalling its `ClassId`.
///
/// NOTE the predicate: `first_dkey < last_user`, NOT `last_dkey < last_user`.
/// Round 1 of this file used the latter, which is wrong — a run of `DKey`s that
/// *straddles* the top of the user range (pizza: first `DKey` at 87, last user
/// class at 95, last `DKey` at 103) is hazardous but reports `false` under it.
fn dkey_sits_below_a_user_class(onto: &SetOntology<RcStr>) -> bool {
    let internal = owl_dl_core::convert::convert_ontology(onto).expect("convert");
    let iris: Vec<String> = (0..internal.vocabulary.num_classes())
        .map(|i| {
            internal
                .vocabulary
                .class_iri(owl_dl_core::ClassId::new(u32::try_from(i).unwrap()))
                .to_string()
        })
        .collect();
    let is_dkey = |s: &String| s.starts_with(owl_dl_core::DKEY_IRI_PREFIX);
    match (
        iris.iter().position(is_dkey),
        iris.iter().rposition(|s| !is_dkey(s)),
    ) {
        (Some(first_dkey), Some(last_user)) => first_dkey < last_user,
        _ => false,
    }
}

/// The whole point of the fixtures: they must actually place a `DKey` BELOW at
/// least one reported class. If a future change to `convert_ontology`'s
/// component ordering pushes every `DKey` to the top of the id space again,
/// the oracles below would still pass — but they would be VACUOUS. This test
/// is the non-vacuity guard, and it is what to look at first if the oracles
/// ever go green for a suspicious reason.
#[test]
fn fixtures_really_put_a_dkey_below_a_user_class() {
    for (name, body) in [
        ("EL", EL_BODY),
        ("hybrid", HYBRID_BODY),
        ("unsat", UNSAT_BODY),
    ] {
        let onto = parse(body);
        assert!(
            dkey_sits_below_a_user_class(&onto),
            "{name} fixture puts every DKey ABOVE every user class, so report \
             position == ClassId and the aliasing oracles below are VACUOUS"
        );
    }
}

/// The corpus counterpart. `bench-corpus/pizza.ofn` exhibits the hazard in its
/// id layout (first `DKey` at id 87, five user classes interned above it, last
/// user class at 95); `mie.ofn` does not (all 84 classes declared, so all 17
/// `DKey`s land at ids 84..=100, above everything).
///
/// Neither can currently FAIL on this bug class — see
/// `inert_declarations_do_not_change_the_hierarchy_pizza` for why pizza's
/// hazard is inert. This test pins the two structural facts so that a future
/// change to either file, or to `convert_ontology`'s ordering, is visible
/// rather than silent.
#[test]
fn pizza_corpus_exhibits_the_hazard_and_mie_does_not() {
    assert!(
        dkey_sits_below_a_user_class(&corpus("pizza.ofn")),
        "pizza.ofn no longer interns a user class above a DKey — the corpus \
         oracle `inert_declarations_do_not_change_the_hierarchy_pizza` is now \
         VACUOUS and needs a new corpus fixture"
    );
    assert!(
        !dkey_sits_below_a_user_class(&corpus("mie.ofn")),
        "mie.ofn now exhibits the hazard — it is worth promoting back to a \
         reproducer (round 1 verified it did NOT, contradicting the original \
         finding)"
    );
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

#[test]
fn classify_agrees_with_direct_query_on_unsat_fixture() {
    let onto = parse(UNSAT_BODY);
    let cls = owl_dl_reasoner::classify(&onto).expect("classify");
    assert_classify_agrees_with_direct_query("classify/unsat", &onto, &cls, true);
}

/// The naive `n²` pair sweep has its own copy of the report-index → `ClassId`
/// mapping, so it needs its own cross-check.
#[test]
fn classify_n2_agrees_with_direct_query() {
    for (label, body) in [
        ("n2/EL", EL_BODY),
        ("n2/hybrid", HYBRID_BODY),
        ("n2/unsat", UNSAT_BODY),
    ] {
        let onto = parse(body);
        let cls = owl_dl_reasoner::classify_n2(&onto).expect("classify_n2");
        assert_classify_agrees_with_direct_query(label, &onto, &cls, true);
    }
}

/// The saturation-only path is a documented sound UNDER-approximation, so only
/// the false-positive direction is asserted.
#[test]
fn classify_saturation_only_has_no_false_positives() {
    for (label, body) in [
        ("sat/EL", EL_BODY),
        ("sat/hybrid", HYBRID_BODY),
        ("sat/unsat", UNSAT_BODY),
    ] {
        let onto = parse(body);
        let cls =
            owl_dl_reasoner::classify_saturation_only(&onto).expect("classify_saturation_only");
        assert_classify_agrees_with_direct_query(label, &onto, &cls, false);
    }
}

#[test]
fn classify_top_down_agrees_with_direct_query() {
    for (label, body) in [
        ("td/EL", EL_BODY),
        ("td/hybrid", HYBRID_BODY),
        ("td/unsat", UNSAT_BODY),
    ] {
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
///
/// This is the strongest oracle in the file: it needs no ground truth, no
/// second reasoner, and it generalises to any future id-assignment-sensitive
/// defect. It is also cheap enough to run on a real corpus file, which the
/// `is_subclass_of` cross-check is NOT (its n² of fresh tableau probes is
/// minutes of work on pizza).
///
/// `classifier` is a parameter so the same property can be pinned on whichever
/// entry point is actually runnable in the profile CI uses — see
/// `inert_declarations_do_not_change_the_hierarchy_pizza_saturation_only`.
fn assert_inert_declarations_are_inert(
    label: &str,
    bare: &SetOntology<RcStr>,
    classifier: fn(
        &SetOntology<RcStr>,
    ) -> Result<owl_dl_reasoner::Classification, owl_dl_reasoner::ReasonError>,
) {
    let declared = with_all_classes_declared(bare);

    let bare_cls = classifier(bare).expect("classify bare");
    let declared_cls = classifier(&declared).expect("classify declared");

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

    let mut bare_unsat = bare_cls.unsatisfiable_classes();
    let mut declared_unsat = declared_cls.unsatisfiable_classes();
    bare_unsat.sort_unstable();
    declared_unsat.sort_unstable();
    assert_eq!(
        bare_unsat, declared_unsat,
        "[{label}] inert declarations changed the UNSATISFIABLE set"
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
         the reported hierarchy — impossible for a correct classifier ({} pair(s), \
         first 40 shown):\n{}",
        differences.len(),
        differences
            .iter()
            .take(40)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
}

#[test]
fn inert_declarations_do_not_change_the_hierarchy_el() {
    assert_inert_declarations_are_inert("EL", &parse(EL_BODY), owl_dl_reasoner::classify);
}

#[test]
fn inert_declarations_do_not_change_the_hierarchy_hybrid() {
    assert_inert_declarations_are_inert("hybrid", &parse(HYBRID_BODY), owl_dl_reasoner::classify);
}

#[test]
fn inert_declarations_do_not_change_the_hierarchy_unsat() {
    assert_inert_declarations_are_inert("unsat", &parse(UNSAT_BODY), owl_dl_reasoner::classify);
}

/// Same three fixtures through `classify_saturation_only`, i.e. through
/// `classify_pure_el` specifically — the function the round-2 unsat-projection
/// false positive lived in.
#[test]
fn inert_declarations_do_not_change_the_saturation_only_hierarchy() {
    for (label, body) in [
        ("sat/EL", EL_BODY),
        ("sat/hybrid", HYBRID_BODY),
        ("sat/unsat", UNSAT_BODY),
    ] {
        assert_inert_declarations_are_inert(
            label,
            &parse(body),
            owl_dl_reasoner::classify_saturation_only,
        );
    }
}

/// The corpus oracle — a LATENT-HAZARD CANARY, not a reproducer. Read this
/// before trusting it.
///
/// `bench-corpus/pizza.ofn` genuinely exhibits the hazard in its ID LAYOUT:
/// five user classes (`pro/ContainedRole`, `pro/DevelopmentRole`,
/// `pro/OnTopPositionRole`, `pro/PersistingRole`, `sulo/Collection`) are
/// interned ABOVE its first `DKey` at id 87 (12 `DKey`s, last user class at 95,
/// 104 class ids). So pre-fix those five report positions did read `DKey` rows.
///
/// But it produces NO OBSERVABLE DELTA, measured: all five are
/// hierarchy-isolated (zero reported subsumers, zero reported subclasses) and
/// pizza's unsatisfiable set is empty, so the rows they misread were empty
/// too. A full hierarchy + unsat dump over `pizza.ofn`, `mie.ofn` and
/// `paper5.ofn` is BYTE-IDENTICAL between `c1f44d8` and the fixed tree (377
/// lines, validated against a sentinel fixture that does differ). This test
/// therefore passes both pre- and post-fix and CANNOT currently fail on this
/// bug class — same practical status as `mie.ofn`, for a different reason
/// (hazard present but inert, vs hazard absent).
///
/// It is kept because it is nearly free (~0.4 s) and because the day pizza's
/// aliased classes gain a hierarchy edge, or an unsatisfiable class appears
/// anywhere in it, this becomes a live oracle on a real ontology with no test
/// change needed. The fixtures above are what actually holds the line today.
///
/// Deliberately the inert-declaration oracle and not the `is_subclass_of`
/// cross-check: the latter's n² of fresh tableau probes does not finish on
/// pizza inside any reasonable test budget.
///
/// # Release-only, and why
///
/// `classify()` on `pizza.ofn` trips a PRE-EXISTING `debug_assert!` in a crate
/// this work does not touch — `crates/owl-dl-tableau/src/hyper.rs:3677`,
/// "≤1 violation reached `find_open_at_most` under `inverse_func_merge`".
/// Verified
/// pre-existing: a bare `classify(pizza.ofn)` panics there on `c1f44d8` too,
/// with none of this branch's code in the picture. Fixing an unrelated tableau
/// invariant is out of scope for a soundness fix to the report projection, and
/// papering over it with `catch_unwind` would hide a real defect. So this test
/// is `ignore`d under `debug_assertions` — it shows up as `ignored` rather than
/// silently absent — and runs for real in `--release`. Delete the gate once
/// hyper.rs:3677 is resolved.
#[test]
#[cfg_attr(
    debug_assertions,
    ignore = "classify(pizza.ofn) trips the pre-existing debug_assert at \
              owl-dl-tableau/src/hyper.rs:3677 (inverse_func_merge); runs in --release"
)]
fn inert_declarations_do_not_change_the_hierarchy_pizza() {
    assert_inert_declarations_are_inert("pizza", &corpus("pizza.ofn"), owl_dl_reasoner::classify);
}

/// The corpus canary that ACTUALLY RUNS IN CI.
///
/// `.github/workflows/ci.yml` runs `cargo test --workspace --all-targets
/// --exclude owl-dl-py` — **debug only, no release job**. So the `classify`
/// variant above, gated off under `debug_assertions` to dodge the pre-existing
/// `hyper.rs:3677` assert, never executes in CI: its "becomes a live oracle the
/// day pizza gains an edge" promise would be empty as configured.
///
/// `classify_saturation_only` fixes that. It does not reach the hypertableau,
/// so it does not trip that assert, and it runs over the same 92-class pizza
/// file in ~0.04 s in debug. Better still, it routes straight through
/// `classify_pure_el` — the exact function the round-2 unsat-projection false
/// positive lived in — so this is the corpus-level guard on the more recently
/// broken path, in the profile CI actually builds.
///
/// Sound to compare for equality even though `classify_saturation_only` is a
/// documented under-approximation: the property asserted is not "the hierarchy
/// is complete" but "adding semantically inert axioms does not change whatever
/// this entry point reports". An under-approximation must still be STABLE under
/// an inert change.
#[test]
fn inert_declarations_do_not_change_the_hierarchy_pizza_saturation_only() {
    assert_inert_declarations_are_inert(
        "pizza/saturation_only",
        &corpus("pizza.ofn"),
        owl_dl_reasoner::classify_saturation_only,
    );
}

/// The unsatisfiable set must name the class that is ACTUALLY `⊑ ⊥`.
///
/// A projection bug here is worse than a wrong subsumption: `Classification::
/// entails` short-circuits on `unsatisfiable_idxs`, so a class wrongly flagged
/// `⊥` is reported as subsuming EVERY other class — an unbounded fan-out of
/// false positives from one mis-indexed bit. Pinned on all four entry points
/// because each builds the unsat set through a different path.
#[test]
fn unsatisfiable_set_names_the_right_class() {
    let onto = parse(UNSAT_BODY);
    let expected = vec!["http://t/Aaa"];
    let variants: Vec<(&str, owl_dl_reasoner::Classification)> = vec![
        (
            "classify",
            owl_dl_reasoner::classify(&onto).expect("classify"),
        ),
        (
            "classify_n2",
            owl_dl_reasoner::classify_n2(&onto).expect("classify_n2"),
        ),
        (
            "classify_saturation_only",
            owl_dl_reasoner::classify_saturation_only(&onto).expect("classify_saturation_only"),
        ),
        (
            "classify_top_down",
            owl_dl_reasoner::classify_top_down_with_timeout(&onto, Duration::from_secs(5))
                .expect("classify_top_down"),
        ),
    ];
    for (name, cls) in &variants {
        let mut unsat = cls.unsatisfiable_classes();
        unsat.sort_unstable();
        assert_eq!(
            unsat, expected,
            "[{name}] wrong unsatisfiable set — a satisfiable class flagged ⊥ \
             subsumes everything, so every pair involving it is a false positive"
        );
    }
}

/// Downstream blast radius: `realize` builds its unsatisfiable-class filter
/// from `classify_saturation_only_internal` (`realize.rs:669`), so the same
/// mis-indexed bit silently deletes a satisfiable class from every
/// individual's entailed types.
#[test]
fn realize_types_survive_the_unsat_projection() {
    let onto = parse(&format!(
        "{UNSAT_BODY}Declaration(NamedIndividual(:i1))\nClassAssertion(:Ccc :i1)\n"
    ));
    let r = owl_dl_reasoner::realize(&onto).expect("realize");
    let types = r.entailed_types("http://t/i1");
    for expected in ["http://t/Ccc", "http://t/Ddd", "http://t/Eee"] {
        assert!(
            types.contains(&expected.to_string()),
            "realize() lost the entailed type {expected} for :i1 — got {types:?}"
        );
    }
    assert!(
        !types.iter().any(|t| t == "http://t/Aaa"),
        "realize() reported the genuinely unsatisfiable Aaa as a type — got {types:?}"
    );
    assert!(
        !types
            .iter()
            .any(|t| t.starts_with(owl_dl_core::DKEY_IRI_PREFIX)),
        "realize() leaked a synthetic DKey IRI into the entailed types — got {types:?}"
    );
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
///
/// # This guard is NOT a proof — know exactly what it misses
///
/// It matches TWO TEXTUAL SPELLINGS. It says nothing about the many ways the
/// same conflation can be written with **no cast at all**: handing a report
/// position to anything that is `ClassId`-indexed under the hood. That is not
/// hypothetical — it is how a live false positive survived round 1 of this
/// fix:
///
/// ```ignore
/// let unsat_bs = closure.unsatisfiable_bitset();  // bit i <=> class_i ⊑ ⊥
/// for i in 0..n {                                // i is a REPORT POSITION
///     if i < unsat_bs.len() && unsat_bs.contains(i) { /* wrong class */ }
/// ```
///
/// No `ClassId::new`, no `as usize`, guard green, hierarchy unsound. Other
/// invisible shapes: indexing any `Vec`/bitset/slice that the saturator or
/// tableau sized and filled by `ClassId`; passing a report position to a
/// function whose `usize` parameter means a class id; arithmetic that
/// reconstructs an id.
///
/// So treat this as a lint against the *known* regression, and treat the
/// behavioural oracles above — the `is_subclass_of` cross-check and especially
/// `assert_inert_declarations_are_inert` — as the actual safety net. Only they
/// caught the bitset bug. Only they can catch the next one.
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
