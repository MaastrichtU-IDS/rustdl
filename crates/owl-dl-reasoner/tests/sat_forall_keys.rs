//! `RUSTDL_SAT_FORALL_KEYS`: `∀`-monotonicity in the EL saturator (#180, `ore_ont_778`).
//!
//! `ore_ont_778` misses `KoalaBear ⊑ HerbivoreAnimal` and `Shark ⊑ CarnivoreAnimal`. Both
//! need `EP ⊑ Plant ⟹ ∀hasRealization.(∀hasPatient.EP ⊓ …) ⊑ ∀hasRealization.(∀hasPatient.Plant
//! ⊓ …)` inside an `∃bearerOf` filler. The saturator dropped any `∀` inside an `∃` filler, and
//! its `∀` keys took only atomic fillers with told monotonicity edges. The tableau proves the
//! pair, but needs ~44 s of search per pair on the full ontology.
//!
//! Asserted at the saturation-closure layer, where the fix lives. Every test locks `ENV`
//! because the flag is read from the process environment and Rust runs tests in parallel.

#![allow(clippy::unwrap_used, clippy::doc_markdown, unsafe_code)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_core::convert::convert_ontology;
use std::io::Cursor;
use std::sync::Mutex;

static ENV: Mutex<()> = Mutex::new(());

fn sat_sub(flag: bool, ont: &str, x: &str, y: &str) -> bool {
    let _g = ENV
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    // SAFETY: every test in this file holds `ENV`, so no other thread reads or writes the
    // environment while it changes.
    unsafe {
        if flag {
            std::env::set_var("RUSTDL_SAT_FORALL_KEYS", "1");
        } else {
            std::env::remove_var("RUSTDL_SAT_FORALL_KEYS");
        }
    }
    let (o, _): (SetOntology<RcStr>, _) = read_ofn(
        &mut Cursor::new(ont.to_string()),
        ParserConfiguration::default(),
    )
    .expect("parse");
    let internal = convert_ontology(&o).expect("lower");
    let subsumers = owl_dl_saturation::saturate(&internal);
    let xid = internal.vocabulary.class_id(x).expect("x");
    let yid = internal.vocabulary.class_id(y).expect("y");
    let r = subsumers.contains(xid, yid);
    unsafe { std::env::remove_var("RUSTDL_SAT_FORALL_KEYS") };
    r
}

/// The `ore_ont_778` shape, reduced. `KOALA_ROLE` is the role of the outer `∀` in `Koala`'s
/// axiom, `KOALA_FILLER` the innermost filler there, `HERB_FILLER` the innermost filler in
/// `Herbivore`'s definition.
fn koala(koala_role: &str, koala_filler: &str, herb_filler: &str) -> String {
    format!(
        r"Prefix(:=<http://e/>)
Ontology(<http://e/o>
Declaration(Class(:Koala)) Declaration(Class(:Herbivore)) Declaration(Class(:EP)) Declaration(Class(:Plant))
Declaration(Class(:Eating)) Declaration(Class(:Disposition)) Declaration(Class(:Animal))
Declaration(ObjectProperty(:bearerOf)) Declaration(ObjectProperty(:hasRealization))
Declaration(ObjectProperty(:hasPatient)) Declaration(ObjectProperty(:other))
SubClassOf(:EP :Plant)
SubClassOf(:Koala :Animal)
SubClassOf(:Koala ObjectSomeValuesFrom(:bearerOf ObjectIntersectionOf(ObjectAllValuesFrom({koala_role} ObjectIntersectionOf(ObjectAllValuesFrom(:hasPatient {koala_filler}) ObjectSomeValuesFrom(:hasPatient {koala_filler}) :Eating)) :Disposition)))
EquivalentClasses(:Herbivore ObjectIntersectionOf(ObjectSomeValuesFrom(:bearerOf ObjectIntersectionOf(ObjectAllValuesFrom(:hasRealization ObjectIntersectionOf(ObjectAllValuesFrom(:hasPatient {herb_filler}) ObjectSomeValuesFrom(:hasPatient {herb_filler}) :Eating)) :Disposition)) :Animal))
)"
    )
}

const K: &str = "http://e/Koala";
const H: &str = "http://e/Herbivore";

#[test]
fn nested_forall_monotonicity_derives_the_subsumption() {
    let ont = koala(":hasRealization", ":EP", ":Plant");
    assert!(sat_sub(true, &ont, K, H), "flag on: Koala ⊑ Herbivore");
}

#[test]
fn the_default_saturator_still_misses_it() {
    // Pins the flag as opt-in. A future default flip must flip this test, not delete it.
    let ont = koala(":hasRealization", ":EP", ":Plant");
    assert!(
        !sat_sub(false, &ont, K, H),
        "flag off: the saturator drops the axioms"
    );
}

#[test]
fn an_unrelated_filler_does_not_subsume() {
    // FP guard: `Animal ⋢ Plant`, so the `∀` keys must not be connected.
    let ont = koala(":hasRealization", ":Animal", ":Plant");
    assert!(!sat_sub(true, &ont, K, H));
}

#[test]
fn monotonicity_does_not_run_backwards() {
    // FP guard: `Plant ⊑ EP` is NOT told, so `∀R.Plant ⋢ ∀R.EP`.
    let ont = koala(":hasRealization", ":Plant", ":EP");
    assert!(!sat_sub(true, &ont, K, H));
}

#[test]
fn a_different_outer_role_does_not_subsume() {
    // FP guard: `∀other.Q ⋢ ∀hasRealization.Q` with no role relation. A rule that keyed the
    // filler without its role would derive this.
    let ont = koala(":other", ":EP", ":Plant");
    assert!(!sat_sub(true, &ont, K, H));
}

#[test]
fn an_inverse_outer_role_is_not_keyed() {
    // `∀hasRealization⁻.Q` is a different restriction; the key is for non-inverse roles only.
    let ont = koala("ObjectInverseOf(:hasRealization)", ":EP", ":Plant");
    assert!(!sat_sub(true, &ont, K, H));
}

#[test]
fn the_atomic_filler_control_is_unchanged() {
    // SP-B2b already handles `∀R.Atomic` as a told RHS and as a conjunct of a definition,
    // with a told monotonicity edge; both arms must agree.
    let ont = r"Prefix(:=<http://e/>)
Ontology(<http://e/o>
Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:K)) Declaration(Class(:L))
Declaration(Class(:Z)) Declaration(ObjectProperty(:r))
SubClassOf(:K :L)
SubClassOf(:A :Z)
SubClassOf(:A ObjectAllValuesFrom(:r :K))
EquivalentClasses(:B ObjectIntersectionOf(ObjectAllValuesFrom(:r :L) :Z))
)";
    for flag in [false, true] {
        assert!(
            sat_sub(flag, ont, "http://e/A", "http://e/B"),
            "flag={flag}"
        );
    }
}

/// Review finding: `∀R.(≥2 s.X)` must not share a key with `∀R.∃s.X`. Konclude and HermiT
/// both refute `Y ⊑ H` here (an `R`-successor with one `s`-successor is a countermodel).
fn forall_min(n: u32) -> String {
    format!(
        r"Prefix(:=<http://e/>)
Ontology(<http://e/o>
Declaration(Class(:H)) Declaration(Class(:Y)) Declaration(Class(:X))
Declaration(ObjectProperty(:r)) Declaration(ObjectProperty(:R)) Declaration(ObjectProperty(:s))
EquivalentClasses(:H ObjectSomeValuesFrom(:r ObjectAllValuesFrom(:R ObjectMinCardinality({n} :s :X))))
SubClassOf(:Y ObjectSomeValuesFrom(:r ObjectAllValuesFrom(:R ObjectSomeValuesFrom(:s :X))))
)"
    )
}

#[test]
fn a_min_two_filler_is_not_read_as_an_existential() {
    assert!(!sat_sub(true, &forall_min(2), "http://e/Y", "http://e/H"));
}

#[test]
fn a_min_one_filler_is_an_existential() {
    // Control: `≥1 s.X ≡ ∃s.X`, so `Y ⊑ H` is entailed and the flag derives it.
    assert!(sat_sub(true, &forall_min(1), "http://e/Y", "http://e/H"));
}
