//! #297 `RUSTDL_SAT_DISTINCT_MIN` (default ON): the EL saturator derives `≥n R.B` from
//! `n` existential facts whose fillers are pairwise provably distinct.
//!
//! `X ⊑ ∃R.{a}`, `a S b`, `a S c`, `DifferentIndividuals(b, c)`, `W ≡ ∃R.(≥2 S)`
//! entails `X ⊑ W`. `classify` missed it while reporting `incomplete: false`: the
//! saturator had no `≥2` key for an unqualified `≥n`, and nothing derived one from two
//! distinct successors. Fillers count as distinct when some class of one is disjoint
//! from some class of the other, or when they are nominals of `DifferentIndividuals`.
//!
//! The nominal cases run through `classify`, where the wedge does not see the
//! assertions (`RUSTDL_CLASSIFY_NOMINAL_OPA` is off). The `TBox` cases run through
//! the saturation-only API, because in `classify` the wedge already derives them.
//! `HermiT` agrees with every verdict below (2026-10-20).
//!
//! Run: `cargo test -p owl-dl-reasoner --test sat_distinct_min`

#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use std::io::Cursor;

// Serialize env mutation; restore on Drop. Every test locks, including the
// read-only ones, so none observes another test's setting.
static ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct SetEnvGuard {
    key: &'static str,
    prior: Option<std::ffi::OsString>,
}
impl SetEnvGuard {
    #[allow(unsafe_code)]
    fn set(key: &'static str, value: &str) -> Self {
        let prior = std::env::var_os(key);
        // SAFETY: set_var is unsafe under edition 2024. Serialized via ENV_MUTEX,
        // restored on Drop.
        unsafe {
            std::env::set_var(key, value);
        }
        Self { key, prior }
    }
}
impl Drop for SetEnvGuard {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        // SAFETY: see set.
        unsafe {
            match &self.prior {
                Some(v) => std::env::set_var(self.key, v),
                None => std::env::remove_var(self.key),
            }
        }
    }
}

const DECLS: &str = "    Declaration(Class(:X)) Declaration(Class(:W)) Declaration(Class(:A))
    Declaration(Class(:B)) Declaration(Class(:C)) Declaration(Class(:D))
    Declaration(ObjectProperty(:R)) Declaration(ObjectProperty(:S))
    Declaration(ObjectProperty(:S1)) Declaration(ObjectProperty(:S2))
    Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b))
    Declaration(NamedIndividual(:c)) Declaration(NamedIndividual(:d))
";

const NOM: &str = "    SubClassOf(:X ObjectSomeValuesFrom(:R ObjectOneOf(:a)))\n";

fn parse(body: &str) -> SetOntology<RcStr> {
    let src = format!(
        "Prefix(:=<http://t/>)\nPrefix(owl:=<http://www.w3.org/2002/07/owl#>)\n\
         Ontology(<http://t/x>\n{DECLS}{body}\n)\n"
    );
    let (onto, _) =
        read_ofn(&mut Cursor::new(src), ParserConfiguration::default()).expect("parse ofn");
    onto
}

/// `classify` with the flag set to `flag`; reports `X ⊑ W`.
fn classify_x_is_w(body: &str, flag: &str) -> bool {
    let _lock = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _g = SetEnvGuard::set("RUSTDL_SAT_DISTINCT_MIN", flag);
    owl_dl_reasoner::classify(&parse(body))
        .expect("classify")
        .is_subclass("http://t/X", "http://t/W")
}

/// The EL closure alone, with the flag set to `flag`; reports `sub ⊑ W`.
fn saturation_sub_is_w(body: &str, sub: &str, flag: &str) -> bool {
    let _lock = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _g = SetEnvGuard::set("RUSTDL_SAT_DISTINCT_MIN", flag);
    owl_dl_reasoner::is_subclass_of_saturation_only(
        &parse(body),
        &format!("http://t/{sub}"),
        "http://t/W",
    )
    .expect("saturation")
}

/// Positive with the flag on, and missed with it off (so the test exercises the rule).
fn derived_only_by_the_rule(holds: impl Fn(&str) -> bool) {
    assert!(holds("1"), "the rule must derive it");
    assert!(!holds("0"), "control: without the rule it is missed");
}

// ---------------------------------------------------------------- nominals

#[test]
fn two_different_individuals_give_min_two() {
    let body = format!(
        "{NOM}    ObjectPropertyAssertion(:S :a :b) ObjectPropertyAssertion(:S :a :c)
    DifferentIndividuals(:b :c)
    EquivalentClasses(:W ObjectSomeValuesFrom(:R ObjectMinCardinality(2 :S)))"
    );
    derived_only_by_the_rule(|f| classify_x_is_w(&body, f));
}

#[test]
fn three_pairwise_different_individuals_give_min_three() {
    let body = format!(
        "{NOM}    ObjectPropertyAssertion(:S :a :b) ObjectPropertyAssertion(:S :a :c)
    ObjectPropertyAssertion(:S :a :d) DifferentIndividuals(:b :c :d)
    EquivalentClasses(:W ObjectSomeValuesFrom(:R ObjectMinCardinality(3 :S)))"
    );
    derived_only_by_the_rule(|f| classify_x_is_w(&body, f));
}

#[test]
fn a_qualified_min_counts_fillers_of_the_qualifier() {
    let body = format!(
        "{NOM}    ObjectPropertyAssertion(:S :a :b) ObjectPropertyAssertion(:S :a :c)
    DifferentIndividuals(:b :c) ClassAssertion(:C :b) ClassAssertion(:C :c)
    EquivalentClasses(:W ObjectSomeValuesFrom(:R ObjectMinCardinality(2 :S :C)))"
    );
    derived_only_by_the_rule(|f| classify_x_is_w(&body, f));
}

#[test]
fn edges_on_two_sub_roles_count_for_the_super_role() {
    let body = format!(
        "{NOM}    SubObjectPropertyOf(:S1 :S) SubObjectPropertyOf(:S2 :S)
    ObjectPropertyAssertion(:S1 :a :b) ObjectPropertyAssertion(:S2 :a :c)
    DifferentIndividuals(:b :c)
    EquivalentClasses(:W ObjectSomeValuesFrom(:R ObjectMinCardinality(2 :S)))"
    );
    derived_only_by_the_rule(|f| classify_x_is_w(&body, f));
}

/// FP guard: without `DifferentIndividuals`, `b` and `c` may be one individual.
#[test]
fn individuals_not_asserted_different_do_not_count_twice() {
    let body = format!(
        "{NOM}    ObjectPropertyAssertion(:S :a :b) ObjectPropertyAssertion(:S :a :c)
    EquivalentClasses(:W ObjectSomeValuesFrom(:R ObjectMinCardinality(2 :S)))"
    );
    assert!(!classify_x_is_w(&body, "1"));
}

/// FP guard: `c` and `d` are not asserted different, so only two are pairwise distinct.
#[test]
fn three_successors_with_one_pair_not_different_are_not_three() {
    let body = format!(
        "{NOM}    ObjectPropertyAssertion(:S :a :b) ObjectPropertyAssertion(:S :a :c)
    ObjectPropertyAssertion(:S :a :d) DifferentIndividuals(:b :c) DifferentIndividuals(:b :d)
    EquivalentClasses(:W ObjectSomeValuesFrom(:R ObjectMinCardinality(3 :S)))"
    );
    assert!(!classify_x_is_w(&body, "1"));
}

/// FP guard: only `b` is known to be a `C`.
#[test]
fn a_filler_outside_the_qualifier_does_not_count() {
    let body = format!(
        "{NOM}    ObjectPropertyAssertion(:S :a :b) ObjectPropertyAssertion(:S :a :c)
    DifferentIndividuals(:b :c) ClassAssertion(:C :b)
    EquivalentClasses(:W ObjectSomeValuesFrom(:R ObjectMinCardinality(2 :S :C)))"
    );
    assert!(!classify_x_is_w(&body, "1"));
}

// ---------------------------------------------------------------- TBox

#[test]
fn two_disjoint_fillers_give_min_two() {
    let body =
        "    SubClassOf(:X ObjectSomeValuesFrom(:S :A)) SubClassOf(:X ObjectSomeValuesFrom(:S :B))
    DisjointClasses(:A :B) EquivalentClasses(:W ObjectMinCardinality(2 :S))";
    derived_only_by_the_rule(|f| saturation_sub_is_w(body, "X", f));
}

#[test]
fn disjoint_fillers_under_the_qualifier_give_a_qualified_min() {
    let body =
        "    SubClassOf(:X ObjectSomeValuesFrom(:S :A)) SubClassOf(:X ObjectSomeValuesFrom(:S :B))
    DisjointClasses(:A :B) SubClassOf(:A :C) SubClassOf(:B :C)
    EquivalentClasses(:W ObjectMinCardinality(2 :S :C))";
    derived_only_by_the_rule(|f| saturation_sub_is_w(body, "X", f));
}

/// The `≥2` sits inside an `∃`, so it is keyed as a body rather than on the left.
#[test]
fn a_nested_min_on_sub_roles_is_derived() {
    let body = "    SubClassOf(:X ObjectSomeValuesFrom(:S1 :A)) SubClassOf(:X ObjectSomeValuesFrom(:S2 :B))
    SubObjectPropertyOf(:S1 :S) SubObjectPropertyOf(:S2 :S) DisjointClasses(:A :B)
    EquivalentClasses(:W ObjectSomeValuesFrom(:R ObjectMinCardinality(2 :S)))
    SubClassOf(:D ObjectSomeValuesFrom(:R :X))";
    derived_only_by_the_rule(|f| saturation_sub_is_w(body, "D", f));
}

/// A told `≥3` is a `≥2`.
#[test]
fn a_told_larger_min_gives_the_smaller_one() {
    let body = "    SubClassOf(:X ObjectMinCardinality(3 :S))
    EquivalentClasses(:W ObjectMinCardinality(2 :S))";
    derived_only_by_the_rule(|f| saturation_sub_is_w(body, "X", f));
}

/// FP guard: `A` and `B` may overlap.
#[test]
fn fillers_not_disjoint_do_not_count_twice() {
    let body =
        "    SubClassOf(:X ObjectSomeValuesFrom(:S :A)) SubClassOf(:X ObjectSomeValuesFrom(:S :B))
    EquivalentClasses(:W ObjectMinCardinality(2 :S))";
    assert!(!saturation_sub_is_w(body, "X", "1"));
}

/// FP guard: the `S1`-successor is not an `S`-successor.
#[test]
fn a_successor_on_an_unrelated_role_does_not_count() {
    let body =
        "    SubClassOf(:X ObjectSomeValuesFrom(:S :A)) SubClassOf(:X ObjectSomeValuesFrom(:S1 :B))
    DisjointClasses(:A :B) EquivalentClasses(:W ObjectMinCardinality(2 :S))";
    assert!(!saturation_sub_is_w(body, "X", "1"));
}

/// FP guard: a told `≥2` is not a `≥3`.
#[test]
fn a_told_smaller_min_does_not_give_a_larger_one() {
    let body = "    SubClassOf(:X ObjectMinCardinality(2 :S))
    EquivalentClasses(:W ObjectMinCardinality(3 :S))";
    assert!(!saturation_sub_is_w(body, "X", "1"));
}
