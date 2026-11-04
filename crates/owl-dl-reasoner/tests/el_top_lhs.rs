//! #444 / #445: ⊤-LHS GCIs in the EL saturator. On main each POSITIVE case below was a
//! missed entailment reported with `completeness_guaranteed: true` (the D10 shape):
//! - #445: `⊤ ⊑ D` kept only `D`'s atomic conjuncts, so `⊤ ⊑ ∃r.B` lost its `∃`;
//! - #444: `seed` broadcast `⊤ ⊑ C` to the user classes only, so a synthetic fact
//!   target (a two-way marker `M ≡ ∃s.B`, a Tseitin synthetic) never got `C`.
//!
//! The reproducers are `~/rustdl/work-435/repro-toplhs/` and #444's; Konclude and
//! `HermiT` agree on every expected answer (their `X ≡ ⊤` is asserted here as
//! `A ⊑ X` for every named `A`, since rustdl reports no ⊤-equivalence).
//!
//! Every positive case asserts the subsumption on BOTH surfaces (classify, which takes
//! the pure-EL saturation path, and the per-pair `is_subclass_of`) and that the file is
//! pure EL with the guarantee on, so a pass is a pass under the claim. The negative
//! cases are FP controls, and two positive-looking cases are marked as controls (they
//! hold on main by another route).
#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::FragmentClassification;
use std::io::Cursor;

const NAMED: [&str; 8] = ["A", "B", "C", "D", "F", "T", "W", "X"];

fn onto(body: &str) -> SetOntology<RcStr> {
    let src = format!(
        "Prefix(:=<http://ex/#>)\nPrefix(owl:=<http://www.w3.org/2002/07/owl#>)\n\
         Ontology(<http://ex/top-lhs>\n\
         Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C)) \
         Declaration(Class(:D)) Declaration(Class(:F)) Declaration(Class(:T)) \
         Declaration(Class(:W)) Declaration(Class(:X)) \
         Declaration(ObjectProperty(:r)) Declaration(ObjectProperty(:s))\n{body}\n)"
    );
    read_ofn(&mut Cursor::new(src), ParserConfiguration::default())
        .expect("parse")
        .0
}

fn iri(name: &str) -> String {
    format!("http://ex/#{name}")
}

/// Every `sub ⊑ sup` pair is derived on both surfaces, under a pure-EL completeness
/// claim, and the ontology is consistent.
fn derives(body: &str, pairs: &[(&str, &str)]) {
    let o = onto(body);
    let c = owl_dl_reasoner::classify(&o).unwrap();
    let stats = c.stats();
    assert_eq!(stats.fragment, FragmentClassification::PureEl, "{body}");
    assert!(stats.completeness_guaranteed(), "{body}: {stats:?}");
    assert!(!stats.inconsistent, "{body}");
    for &(sub, sup) in pairs {
        assert!(
            c.is_subclass(&iri(sub), &iri(sup)),
            "classify misses {sub} ⊑ {sup}:\n{body}"
        );
        assert!(
            owl_dl_reasoner::is_subclass_of(&o, &iri(sub), &iri(sup)).unwrap(),
            "subclass misses {sub} ⊑ {sup}:\n{body}"
        );
    }
}

/// `X ≡ ⊤`, read as: every named class is `⊑ X`.
fn everything_below(x: &str) -> Vec<(&'static str, &str)> {
    NAMED.iter().map(|&a| (a, x)).collect()
}

/// `sub ⊑ sup` is NOT derived on either surface (an FP control).
fn does_not_derive(body: &str, sub: &str, sup: &str) {
    let o = onto(body);
    let c = owl_dl_reasoner::classify(&o).unwrap();
    assert!(
        !c.is_subclass(&iri(sub), &iri(sup)),
        "classify over-derives {sub} ⊑ {sup}:\n{body}"
    );
    assert!(
        !owl_dl_reasoner::is_subclass_of(&o, &iri(sub), &iri(sup)).unwrap(),
        "subclass over-derives {sub} ⊑ {sup}:\n{body}"
    );
}

// ---- #445: the ⊤-LHS consequent lowers in full ------------------------------------

/// `topex`: `⊤ ⊑ ∃r.B`, `∃r.B ⊑ X` ⟹ `X ≡ ⊤`.
#[test]
fn topex() {
    derives(
        "SubClassOf(owl:Thing ObjectSomeValuesFrom(:r :B))
         SubClassOf(ObjectSomeValuesFrom(:r :B) :X)",
        &everything_below("X"),
    );
}

/// `topand`: `⊤ ⊑ C ⊓ ∃r.B`, `∃r.B ⊑ X` ⟹ `X ≡ C ≡ ⊤`.
#[test]
fn topand() {
    let body = "SubClassOf(owl:Thing ObjectIntersectionOf(:C ObjectSomeValuesFrom(:r :B)))
                SubClassOf(ObjectSomeValuesFrom(:r :B) :X)";
    derives(body, &everything_below("X"));
    derives(body, &everything_below("C"));
}

/// `toptop`: `⊤ ⊑ ∃r.⊤`, `Domain(r, D)` ⟹ `D ≡ ⊤`.
#[test]
fn toptop() {
    derives(
        "SubClassOf(owl:Thing ObjectSomeValuesFrom(:r owl:Thing))
         ObjectPropertyDomain(:r :D)",
        &everything_below("D"),
    );
}

/// `topeq`: `⊤ ≡ ∃r.B`, `∃r.B ⊑ X` ⟹ `X ≡ ⊤`.
#[test]
fn topeq() {
    derives(
        "EquivalentClasses(owl:Thing ObjectSomeValuesFrom(:r :B))
         SubClassOf(ObjectSomeValuesFrom(:r :B) :X)",
        &everything_below("X"),
    );
}

/// The consequence lands as a scored pair, not only as `⊤`-equivalence:
/// `⊤ ⊑ ∃r.B`, `A ⊓ ∃r.B ⊑ X` ⟹ `A ⊑ X` (and not `B ⊑ X`).
#[test]
fn top_existential_completes_a_conjunctive_trigger() {
    let body = "SubClassOf(owl:Thing ObjectSomeValuesFrom(:r :B))
                SubClassOf(ObjectIntersectionOf(:A ObjectSomeValuesFrom(:r :B)) :X)";
    derives(body, &[("A", "X")]);
    does_not_derive(body, "B", "X");
}

/// A nested consequent: `⊤ ⊑ ∃r.∃s.B`, `∃r.∃s.B ⊑ X` ⟹ `X ≡ ⊤`.
#[test]
fn nested_top_existential() {
    derives(
        "SubClassOf(owl:Thing ObjectSomeValuesFrom(:r ObjectSomeValuesFrom(:s :B)))
         SubClassOf(ObjectSomeValuesFrom(:r ObjectSomeValuesFrom(:s :B)) :X)",
        &everything_below("X"),
    );
}

/// The ⊤ key's successor carries the range: `⊤ ⊑ ∃r.B`, `Range(r, F)`,
/// `∃r.(B ⊓ F) ⊑ X` ⟹ `X ≡ ⊤`.
#[test]
fn top_existential_with_a_range() {
    derives(
        "SubClassOf(owl:Thing ObjectSomeValuesFrom(:r :B)) ObjectPropertyRange(:r :F)
         SubClassOf(ObjectSomeValuesFrom(:r ObjectIntersectionOf(:B :F)) :X)",
        &everything_below("X"),
    );
}

/// `⊤ ⊑ ∃r.B` with `B ⊑ ⊥` is inconsistent. On main classify said `consistent: true`
/// under the guarantee; the `consistent` command (tableau) already said inconsistent.
#[test]
fn an_empty_top_filler_is_inconsistent() {
    let o = onto(
        "SubClassOf(owl:Thing ObjectSomeValuesFrom(:r :B))
         SubClassOf(:B owl:Nothing)",
    );
    let c = owl_dl_reasoner::classify(&o).unwrap();
    assert!(c.stats().inconsistent);
    assert!(!owl_dl_reasoner::is_consistent(&o).unwrap());
}

/// The control: a told `⊤ ⊑ X` is unchanged.
#[test]
fn control_told_top_subsumer() {
    derives("SubClassOf(owl:Thing :X)", &everything_below("X"));
}

// ---- #444: synthetic fact targets get `⊤ ⊑ C` ---------------------------------------

/// #444's reproducer: `X ⊑ ∃r.M` for the two-way marker `M ≡ ∃s.B`; `M ⊑ T` only via
/// `⊤ ⊑ T`, so `∃r.T ⊑ W` gives `X ⊑ W`.
#[test]
fn a_marker_target_gets_top_subsumers() {
    derives(
        "SubClassOf(owl:Thing :T)
         SubClassOf(:X ObjectSomeValuesFrom(:r ObjectSomeValuesFrom(:s :B)))
         SubClassOf(ObjectSomeValuesFrom(:r :T) :W)",
        &[("X", "W")],
    );
}

/// A control, true on main too: with `Range(r, F)` the target is the Tseitin `M ⊓ F`,
/// which already reached `T` through its user conjunct `F` (sabotaging the #444
/// broadcast leaves it green). Kept so the broadcast change cannot break it.
#[test]
fn a_tseitin_target_gets_top_subsumers() {
    derives(
        "SubClassOf(owl:Thing :T) ObjectPropertyRange(:r :F)
         SubClassOf(:X ObjectSomeValuesFrom(:r ObjectSomeValuesFrom(:s :B)))
         SubClassOf(ObjectSomeValuesFrom(:r :T) :W)",
        &[("X", "W")],
    );
}

/// Both bugs at once: the ⊤ key's own facts reach a marker target.
/// `⊤ ⊑ ∃s.D`, `X ⊑ ∃r.∃s.B`, `∃r.∃s.D ⊑ W` ⟹ `X ⊑ W`.
#[test]
fn the_top_key_reaches_a_marker_target() {
    derives(
        "SubClassOf(owl:Thing ObjectSomeValuesFrom(:s :D))
         SubClassOf(:X ObjectSomeValuesFrom(:r ObjectSomeValuesFrom(:s :B)))
         SubClassOf(ObjectSomeValuesFrom(:r ObjectSomeValuesFrom(:s :D)) :W)",
        &[("X", "W")],
    );
}

/// A nominal stand-in is a synthetic too: `X ⊑ ∃s.{a}`, `⊤ ⊑ ∃r.B`, `∃s.∃r.B ⊑ W` ⟹
/// `X ⊑ W`, and with `⊤ ⊑ T`, `∃s.T ⊑ Y` ⟹ `X ⊑ Y`. Nominals are outside the pure-EL
/// gate, so this checks the answer on both surfaces whichever engine gives it. A control:
/// the tableau answers it, so it holds on main too.
#[test]
fn a_nominal_stand_in_gets_the_top_axioms() {
    let o = onto(
        "Declaration(NamedIndividual(:a)) Declaration(Class(:Y))
         SubClassOf(:X ObjectSomeValuesFrom(:s ObjectOneOf(:a)))
         SubClassOf(owl:Thing ObjectSomeValuesFrom(:r :B))
         SubClassOf(ObjectSomeValuesFrom(:s ObjectSomeValuesFrom(:r :B)) :W)
         SubClassOf(owl:Thing :T)
         SubClassOf(ObjectSomeValuesFrom(:s :T) :Y)",
    );
    let c = owl_dl_reasoner::classify(&o).unwrap();
    for sup in ["W", "Y"] {
        assert!(
            c.is_subclass(&iri("X"), &iri(sup)),
            "classify misses X ⊑ {sup}"
        );
        assert!(
            owl_dl_reasoner::is_subclass_of(&o, &iri("X"), &iri(sup)).unwrap(),
            "subclass misses X ⊑ {sup}"
        );
    }
}

// ---- FP controls --------------------------------------------------------------------

/// `⊤ ⊑ ∃r.B` says nothing about another role, or another filler.
#[test]
fn a_top_existential_stays_on_its_role_and_filler() {
    let other_role = "SubClassOf(owl:Thing ObjectSomeValuesFrom(:r :B))
                      SubClassOf(ObjectSomeValuesFrom(:s :B) :X)";
    does_not_derive(other_role, "A", "X");
    let other_filler = "SubClassOf(owl:Thing ObjectSomeValuesFrom(:r :B))
                        SubClassOf(ObjectSomeValuesFrom(:r :C) :X)";
    does_not_derive(other_filler, "A", "X");
}

/// `⊤ ⊑ T` gives the marker `T`, never the marker's own filler.
#[test]
fn the_broadcast_adds_nothing_but_the_top_subsumers() {
    does_not_derive(
        "SubClassOf(owl:Thing :T)
         SubClassOf(:X ObjectSomeValuesFrom(:r ObjectSomeValuesFrom(:s :B)))
         SubClassOf(ObjectSomeValuesFrom(:r :C) :W)",
        "X",
        "W",
    );
}
