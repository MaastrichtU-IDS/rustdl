//! #532 / #535: the consistency wedge's trusted `Sat` accepted inconsistent `ABox`es,
//! because its input (`ConsistencyCache::build`) lacked an axiom kind (#532:
//! `NegativeObjectPropertyAssertion`) or under-enforced one it had (#535: a reflexive
//! role under `≤0`; `InverseFunctional` spelled through an `InverseObjectProperties`
//! alias). Every ontology here was decided by Konclude AND `HermiT`; the expected
//! verdict is theirs. The consistent controls come first: the fix adds constraints,
//! so a false `inconsistent` is the failure that matters most.
//!
//! Sabotage: `RUSTDL_WEDGE_ABOX_COVERAGE=0` (negative assertions, inverse-functional
//! alias) and `RUSTDL_REFLEXIVE_MAX_ZERO_LOOP=0` (reflexive `≤0`) make the matching
//! inconsistent cases below read `consistent` again (see `flags_revert_each_fix`).
//!
//! Every test locks the env mutex, since some set flags.

#![allow(clippy::unwrap_used)]

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::RcStr;
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::{QueryStats, is_consistent_with_stats};
use std::io::Cursor;

static ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn lock() -> std::sync::MutexGuard<'static, ()> {
    ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

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
        unsafe { std::env::set_var(key, value) };
        Self { key, prior }
    }
}
impl Drop for SetEnvGuard {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        // SAFETY: see `set`.
        unsafe {
            match &self.prior {
                Some(v) => std::env::set_var(self.key, v),
                None => std::env::remove_var(self.key),
            }
        }
    }
}

const DECLS: &str = "Declaration(ObjectProperty(:r)) Declaration(ObjectProperty(:s)) \
     Declaration(ObjectProperty(:r0)) Declaration(ObjectProperty(:r1)) \
     Declaration(Class(:Q)) Declaration(Class(:Q1)) Declaration(Class(:Q2)) \
     Declaration(Class(:C2)) Declaration(Class(:C3)) Declaration(Class(:C4)) \
     Declaration(Class(:C7)) Declaration(Class(:P)) Declaration(DataProperty(:d)) Declaration(DataProperty(:e)) \
     Declaration(NamedIndividual(:a)) Declaration(NamedIndividual(:b)) \
     Declaration(NamedIndividual(:c))";

fn consistent(body: &str) -> (bool, QueryStats) {
    let ofn = format!(
        "Prefix(:=<http://ex/#>)\n\
         Prefix(owl:=<http://www.w3.org/2002/07/owl#>)\n\
         Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)\n\
         Ontology(<http://ex/wedgecoverage>\n{DECLS}\n{body}\n)\n"
    );
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut Cursor::new(ofn), ParserConfiguration::default()).expect("parse ofn");
    is_consistent_with_stats(&onto).expect("consistent")
}

fn assert_consistent(body: &str) {
    let (verdict, stats) = consistent(body);
    assert!(
        verdict,
        "Konclude and HermiT: consistent. {stats:?}\n{body}"
    );
}

fn assert_inconsistent(body: &str) {
    let (verdict, stats) = consistent(body);
    assert!(
        !verdict,
        "Konclude and HermiT: inconsistent. {stats:?}\n{body}"
    );
}

// ---- consistent controls (negatives first) -------------------------------------

/// `¬r(a, b)` with `r` reflexive: only `r(a, a)` is forced.
const K_REFL_OTHER: &str = "ReflexiveObjectProperty(:r) NegativeObjectPropertyAssertion(:r :a :b)";
/// Symmetry gives `r(c, b)`, not `r(a, b)`.
const K_SYM_NO_EDGE: &str = "SymmetricObjectProperty(:r) ObjectPropertyAssertion(:r :b :c) \
     NegativeObjectPropertyAssertion(:r :a :b)";
const K_HASVALUE_OTHER: &str = "ClassAssertion(ObjectHasValue(:r :b) :a) \
     NegativeObjectPropertyAssertion(:r :a :c)";
const K_SAME_OTHER: &str = "ObjectPropertyAssertion(:r :a :b) SameIndividual(:a :c) \
     NegativeObjectPropertyAssertion(:r :b :a)";
/// Transitivity gives `r(a, c)`, not `r(c, a)`.
const K_TRANS_REVERSED: &str = "TransitiveObjectProperty(:r) ObjectPropertyAssertion(:r :a :b) \
     ObjectPropertyAssertion(:r :b :c) NegativeObjectPropertyAssertion(:r :c :a)";
/// `s(a, b)` with `s ≡ r⁻` is `r(b, a)`, not `r(a, b)`.
const K_INVERSE_DIRECTION: &str = "InverseObjectProperties(:r :s) \
     ObjectPropertyAssertion(:s :a :b) NegativeObjectPropertyAssertion(:r :a :b)";
const K_NEG_INVERSE_OK: &str = "ObjectPropertyAssertion(:r :a :b) \
     NegativeObjectPropertyAssertion(ObjectInverseOf(:r) :a :b)";
/// Reflexive `r` under `≤1`: the self-loop is the one successor allowed.
const K_REFL_MAX1: &str =
    "ReflexiveObjectProperty(:r) ClassAssertion(ObjectMaxCardinality(1 :r) :a)";
/// Reflexive `r` under `≤0 r.Q`: satisfied by `a ∉ Q`.
const K_REFL_MAX0_QUALIFIED: &str =
    "ReflexiveObjectProperty(:r) ClassAssertion(ObjectMaxCardinality(0 :r :Q) :a)";
/// The alias case without the disjointness: the two successors merge harmlessly.
const K_IF_ALIAS_NO_CLASH: &str = "InverseObjectProperties(:r :s) \
     InverseFunctionalObjectProperty(:s) \
     ClassAssertion(ObjectIntersectionOf(ObjectSomeValuesFrom(:r :Q1) \
     ObjectSomeValuesFrom(:r :Q2)) :a)";
/// `InverseFunctional(s)` bounds `s`-PREDECESSORS; two `s`-successors are fine.
const K_IF_ALIAS_SUCCESSORS: &str = "InverseObjectProperties(:r :s) \
     InverseFunctionalObjectProperty(:s) DisjointClasses(:Q1 :Q2) \
     ClassAssertion(ObjectIntersectionOf(ObjectSomeValuesFrom(:s :Q1) \
     ObjectSomeValuesFrom(:s :Q2)) :a)";
const K_REFL_PREDECESSOR_MAX1: &str = "ReflexiveObjectProperty(:r) \
     ObjectPropertyAssertion(:r :b :a) \
     ClassAssertion(ObjectMaxCardinality(1 ObjectInverseOf(:r)) :a)";
const K_NEG_DATA_OTHER_VALUE: &str = "DataPropertyAssertion(:d :a \"2\"^^xsd:integer) \
     NegativeDataPropertyAssertion(:d :a \"1\"^^xsd:integer)";

#[test]
fn control_negative_assertion_reflexive_other_individual() {
    let _g = lock();
    assert_consistent(K_REFL_OTHER);
}
#[test]
fn control_negative_assertion_symmetric_unrelated_edge() {
    let _g = lock();
    assert_consistent(K_SYM_NO_EDGE);
}
#[test]
fn control_negative_assertion_has_value_other_individual() {
    let _g = lock();
    assert_consistent(K_HASVALUE_OTHER);
}
#[test]
fn control_negative_assertion_same_individual_other_pair() {
    let _g = lock();
    assert_consistent(K_SAME_OTHER);
}
#[test]
fn control_negative_assertion_transitive_reversed() {
    let _g = lock();
    assert_consistent(K_TRANS_REVERSED);
}
#[test]
fn control_negative_assertion_inverse_alias_other_direction() {
    let _g = lock();
    assert_consistent(K_INVERSE_DIRECTION);
}
#[test]
fn control_negative_inverse_assertion_other_direction() {
    let _g = lock();
    assert_consistent(K_NEG_INVERSE_OK);
}
#[test]
fn control_reflexive_max_one() {
    let _g = lock();
    assert_consistent(K_REFL_MAX1);
}
#[test]
fn control_reflexive_qualified_max_zero() {
    let _g = lock();
    assert_consistent(K_REFL_MAX0_QUALIFIED);
}
#[test]
fn control_inverse_functional_alias_without_clash() {
    let _g = lock();
    assert_consistent(K_IF_ALIAS_NO_CLASH);
}
#[test]
fn control_inverse_functional_alias_successors_unbounded() {
    let _g = lock();
    assert_consistent(K_IF_ALIAS_SUCCESSORS);
}
#[test]
fn control_reflexive_predecessor_max_one_same() {
    let _g = lock();
    assert_consistent(K_REFL_PREDECESSOR_MAX1);
}
#[test]
fn control_negative_data_assertion_other_value() {
    let _g = lock();
    assert_consistent(K_NEG_DATA_OTHER_VALUE);
}

// ---- #532: negative object property assertions --------------------------------

const N_REFLEXIVE: &str = "ReflexiveObjectProperty(:r) NegativeObjectPropertyAssertion(:r :a :a)";
const N_SYMMETRIC: &str = "SymmetricObjectProperty(:r) ObjectPropertyAssertion(:r :b :a) \
     NegativeObjectPropertyAssertion(:r :a :b)";
const N_HAS_VALUE: &str = "ClassAssertion(ObjectHasValue(:r :b) :a) \
     NegativeObjectPropertyAssertion(:r :a :b)";
const N_SAME_INDIVIDUAL: &str = "ObjectPropertyAssertion(:r :a :b) SameIndividual(:a :b) \
     NegativeObjectPropertyAssertion(:r :a :a)";
const N_TRANSITIVE: &str = "TransitiveObjectProperty(:r) ObjectPropertyAssertion(:r :a :b) \
     ObjectPropertyAssertion(:r :b :c) NegativeObjectPropertyAssertion(:r :a :c)";
/// #532 comment: `s(b, a)` with `s ≡ r⁻` is `r(a, b)`.
const N_INVERSE_ALIAS: &str = "InverseObjectProperties(:r :s) \
     ObjectPropertyAssertion(:s :b :a) NegativeObjectPropertyAssertion(:r :a :b)";
/// Fuzz core 1: `r0 ⊑ r1 ≡ r0⁻` makes `r0` symmetric.
const N_FUZZ_SYMMETRIC_BY_INVERSE: &str = "InverseObjectProperties(:r0 :r1) \
     SubObjectPropertyOf(:r0 :r1) NegativeObjectPropertyAssertion(:r0 :a :b) \
     ClassAssertion(ObjectSomeValuesFrom(:r1 ObjectOneOf(:b)) :a)";
/// Fuzz core 2: `a = c` (from `{c, b}` and `b ≠ a`), reflexivity gives `r0⁻(c, c)`.
const N_FUZZ_REFLEXIVE_NOMINAL: &str = "NegativeObjectPropertyAssertion(ObjectInverseOf(:r0) :a :c) \
     ClassAssertion(ObjectOneOf(:c :b) :a) DifferentIndividuals(:b :a) \
     ReflexiveObjectProperty(ObjectInverseOf(:r0))";
/// Caught before the fix (by `abox_check` P3); kept as controls of the same surface.
const N_P3_SUB_ROLE: &str = "SubObjectPropertyOf(:s :r) ObjectPropertyAssertion(:s :a :b) \
     NegativeObjectPropertyAssertion(:r :a :b)";
const N_P3_INVERSE: &str = "ObjectPropertyAssertion(:r :b :a) \
     NegativeObjectPropertyAssertion(ObjectInverseOf(:r) :a :b)";

#[test]
fn negative_assertion_against_reflexive_self_edge() {
    let _g = lock();
    assert_inconsistent(N_REFLEXIVE);
}
#[test]
fn negative_assertion_against_symmetric_edge() {
    let _g = lock();
    assert_inconsistent(N_SYMMETRIC);
}
#[test]
fn negative_assertion_against_has_value() {
    let _g = lock();
    assert_inconsistent(N_HAS_VALUE);
}
#[test]
fn negative_assertion_against_same_individual_merge() {
    let _g = lock();
    assert_inconsistent(N_SAME_INDIVIDUAL);
}
#[test]
fn negative_assertion_against_transitive_edge() {
    let _g = lock();
    assert_inconsistent(N_TRANSITIVE);
}
#[test]
fn negative_assertion_against_inverse_alias_edge() {
    let _g = lock();
    assert_inconsistent(N_INVERSE_ALIAS);
}
#[test]
fn negative_assertion_fuzz_core_symmetric_by_inverse() {
    let _g = lock();
    assert_inconsistent(N_FUZZ_SYMMETRIC_BY_INVERSE);
}
#[test]
fn negative_assertion_fuzz_core_reflexive_nominal() {
    let _g = lock();
    assert_inconsistent(N_FUZZ_REFLEXIVE_NOMINAL);
}
#[test]
fn negative_assertion_against_sub_role_edge() {
    let _g = lock();
    assert_inconsistent(N_P3_SUB_ROLE);
}
#[test]
fn negative_inverse_assertion_against_edge() {
    let _g = lock();
    assert_inconsistent(N_P3_INVERSE);
}

// ---- #535: reflexive `≤0`, inverse-functional through an alias ------------------

const M_REFLEXIVE_MAX0: &str =
    "ReflexiveObjectProperty(:r) ClassAssertion(ObjectMaxCardinality(0 :r) :a)";
const M_REFLEXIVE_INVERSE_MAX0: &str = "ReflexiveObjectProperty(ObjectInverseOf(:r)) \
     ClassAssertion(ObjectMaxCardinality(0 :r) :a)";
/// Caught before the fix: the `∀` form sees the self-edge.
const M_REFLEXIVE_INVERSE_ALL_BOTTOM: &str = "ReflexiveObjectProperty(ObjectInverseOf(:r)) \
     ClassAssertion(ObjectAllValuesFrom(:r owl:Nothing) :a)";
const M_FUZZ_REFLEXIVE: &str = "ReflexiveObjectProperty(ObjectInverseOf(:r0)) \
     ClassAssertion(ObjectAllValuesFrom(:r0 :C4) :a) \
     EquivalentClasses(:C4 ObjectExactCardinality(0 :r0 :C7)) SubClassOf(owl:Thing :C7)";
const M_REFLEXIVE_PREDECESSOR_MAX1: &str = "ReflexiveObjectProperty(:r) \
     ObjectPropertyAssertion(:r :b :a) \
     ClassAssertion(ObjectMaxCardinality(1 ObjectInverseOf(:r)) :a) DifferentIndividuals(:a :b)";
const M_IF_ALIAS: &str = "InverseObjectProperties(:r :s) InverseFunctionalObjectProperty(:s) \
     DisjointClasses(:Q1 :Q2) \
     ClassAssertion(ObjectIntersectionOf(ObjectSomeValuesFrom(:r :Q1) \
     ObjectSomeValuesFrom(:r :Q2)) :a)";
/// Caught before the fix: written directly, conversion normalises it to `Functional(r)`.
const M_IF_INVERSE_DIRECT: &str = "InverseFunctionalObjectProperty(ObjectInverseOf(:r)) \
     DisjointClasses(:Q1 :Q2) \
     ClassAssertion(ObjectIntersectionOf(ObjectSomeValuesFrom(:r :Q1) \
     ObjectSomeValuesFrom(:r :Q2)) :a)";
const M_FUNCTIONAL: &str = "FunctionalObjectProperty(:r) DisjointClasses(:Q1 :Q2) \
     ClassAssertion(ObjectIntersectionOf(ObjectSomeValuesFrom(:r :Q1) \
     ObjectSomeValuesFrom(:r :Q2)) :a)";
const M_FUZZ_IF_ALIAS: &str = "InverseObjectProperties(:r0 :r1) \
     InverseFunctionalObjectProperty(:r1) \
     SubClassOf(:C4 ObjectIntersectionOf(ObjectSomeValuesFrom(:r0 :C2) \
     ObjectSomeValuesFrom(:r0 :C3))) DisjointClasses(:C2 :C3) ClassAssertion(:C4 :a)";

#[test]
fn reflexive_role_under_max_zero() {
    let _g = lock();
    assert_inconsistent(M_REFLEXIVE_MAX0);
}
#[test]
fn reflexive_inverse_role_under_max_zero() {
    let _g = lock();
    assert_inconsistent(M_REFLEXIVE_INVERSE_MAX0);
}
#[test]
fn reflexive_inverse_role_under_all_bottom() {
    let _g = lock();
    assert_inconsistent(M_REFLEXIVE_INVERSE_ALL_BOTTOM);
}
#[test]
fn reflexive_fuzz_core_exact_zero() {
    let _g = lock();
    assert_inconsistent(M_FUZZ_REFLEXIVE);
}
#[test]
fn reflexive_predecessor_under_max_one() {
    let _g = lock();
    assert_inconsistent(M_REFLEXIVE_PREDECESSOR_MAX1);
}
#[test]
fn inverse_functional_through_inverse_alias() {
    let _g = lock();
    assert_inconsistent(M_IF_ALIAS);
}
#[test]
fn inverse_functional_written_on_inverse() {
    let _g = lock();
    assert_inconsistent(M_IF_INVERSE_DIRECT);
}
#[test]
fn functional_two_disjoint_successors() {
    let _g = lock();
    assert_inconsistent(M_FUNCTIONAL);
}
#[test]
fn inverse_functional_fuzz_core_alias() {
    let _g = lock();
    assert_inconsistent(M_FUZZ_IF_ALIAS);
}

// ---- found by the coverage audit (not in either issue) -------------------------
// Each inconsistent by Konclude and `HermiT`; each read `consistent`, trusted, on main.

/// `InverseFunctional(r)` + `Symmetric(r)` is `Functional(r)`; the census behind the
/// `≤1 r⁻` GCI counts only `∃r⁻` generators, so the wedge had no bound.
const A_IF_SYMMETRIC: &str = "InverseFunctionalObjectProperty(:r) SymmetricObjectProperty(:r) \
     DisjointClasses(:Q1 :Q2) \
     ClassAssertion(ObjectIntersectionOf(ObjectSomeValuesFrom(:r :Q1) \
     ObjectSomeValuesFrom(:r :Q2)) :a)";
const A_SELF_NEGATIVE: &str =
    "ClassAssertion(ObjectHasSelf(:r) :a) NegativeObjectPropertyAssertion(:r :a :a)";
const A_CHAIN_NEGATIVE: &str = "SubObjectPropertyOf(ObjectPropertyChain(:r :s) :t) \
     ObjectPropertyAssertion(:r :a :b) ObjectPropertyAssertion(:s :b :c) \
     NegativeObjectPropertyAssertion(:t :a :c)";
const A_RANGE_NOMINAL_NEGATIVE: &str = "ObjectPropertyRange(:r ObjectOneOf(:b)) \
     ClassAssertion(ObjectSomeValuesFrom(:r owl:Thing) :a) \
     NegativeObjectPropertyAssertion(:r :a :b)";
/// Consistent: `InverseFunctional(s)`, `s ≡ r⁻` is `Functional(r)`, and two
/// `r`-PREDECESSORS of `b` do not violate it.
const A_IF_ALIAS_PREDECESSORS: &str = "InverseObjectProperties(:r :s) \
     InverseFunctionalObjectProperty(:s) ObjectPropertyAssertion(:r :a :b) \
     ObjectPropertyAssertion(:r :c :b) DifferentIndividuals(:a :c)";

#[test]
fn control_inverse_functional_alias_two_predecessors() {
    let _g = lock();
    assert_consistent(A_IF_ALIAS_PREDECESSORS);
}
#[test]
fn inverse_functional_symmetric_two_disjoint_successors() {
    let _g = lock();
    assert_inconsistent(A_IF_SYMMETRIC);
}
#[test]
fn negative_assertion_against_has_self() {
    let _g = lock();
    assert_inconsistent(A_SELF_NEGATIVE);
}
#[test]
fn negative_assertion_against_chain_edge() {
    let _g = lock();
    assert_inconsistent(A_CHAIN_NEGATIVE);
}
#[test]
fn negative_assertion_against_nominal_range() {
    let _g = lock();
    assert_inconsistent(A_RANGE_NOMINAL_NEGATIVE);
}

// ---- NegativeDataPropertyAssertion (lowered to a `ClassAssertion`) -------------

#[test]
fn negative_data_assertion_against_asserted_value() {
    let _g = lock();
    assert_inconsistent(
        "DataPropertyAssertion(:d :a \"1\"^^xsd:integer) \
         NegativeDataPropertyAssertion(:d :a \"1\"^^xsd:integer)",
    );
}
#[test]
fn negative_data_assertion_against_sub_property_value() {
    let _g = lock();
    assert_inconsistent(
        "SubDataPropertyOf(:e :d) DataPropertyAssertion(:e :a \"1\"^^xsd:integer) \
         NegativeDataPropertyAssertion(:d :a \"1\"^^xsd:integer)",
    );
}

// ---- OPEN: `≤0 r.C` with a complex qualifier `C` (found in review of #532/#535) ---
// Not fixed here. The wedge does not check a successor (or a reflexive self-loop)
// against a union or intersection qualifier under `≤0`, so each case below reads
// `consistent`, trusted, on main and with this fix. Konclude, `HermiT` and the main
// tableau (`RUSTDL_WEDGE_CONSISTENCY=0`) all say inconsistent. The `#[ignore]`d
// sentinels assert the CORRECT answer: one that starts passing is a finding.

const U1_EDGE_TAUTOLOGY: &str = "ObjectPropertyAssertion(:r :a :b) \
     ClassAssertion(ObjectMaxCardinality(0 :r ObjectUnionOf(:Q ObjectComplementOf(:Q))) :a)";
const U2_REFL_UNION_TYPED: &str = "ReflexiveObjectProperty(:r) \
     ClassAssertion(ObjectMaxCardinality(0 :r ObjectUnionOf(:Q :P)) :a) ClassAssertion(:P :a)";
const U3_REFL_TAUTOLOGY: &str = "ReflexiveObjectProperty(:r) \
     ClassAssertion(ObjectMaxCardinality(0 :r ObjectUnionOf(:Q ObjectComplementOf(:Q))) :a)";
const U4_REFL_INTERSECTION_TYPED: &str = "ReflexiveObjectProperty(:r) \
     ClassAssertion(ObjectMaxCardinality(0 :r ObjectIntersectionOf(:Q :P)) :a) \
     ClassAssertion(:P :a) ClassAssertion(:Q :a)";
const U5_EDGE_UNION_TYPED: &str = "ObjectPropertyAssertion(:r :a :b) \
     ClassAssertion(ObjectMaxCardinality(0 :r ObjectUnionOf(:Q :P)) :a) ClassAssertion(:P :b)";

#[test]
fn control_complex_qualifier_max_zero_no_edge() {
    let _g = lock();
    assert_consistent(
        "ClassAssertion(ObjectMaxCardinality(0 :r ObjectUnionOf(:Q ObjectComplementOf(:Q))) :a)",
    );
}
#[test]
fn control_complex_qualifier_reflexive_union_untyped() {
    let _g = lock();
    assert_consistent(
        "ReflexiveObjectProperty(:r) \
         ClassAssertion(ObjectMaxCardinality(0 :r ObjectUnionOf(:Q :P)) :a)",
    );
}
#[test]
fn control_complex_qualifier_reflexive_intersection_half_typed() {
    let _g = lock();
    assert_consistent(
        "ReflexiveObjectProperty(:r) \
         ClassAssertion(ObjectMaxCardinality(0 :r ObjectIntersectionOf(:Q :P)) :a) \
         ClassAssertion(:Q :a)",
    );
}
#[test]
fn control_complex_qualifier_edge_untyped() {
    let _g = lock();
    assert_consistent(
        "ObjectPropertyAssertion(:r :a :b) \
         ClassAssertion(ObjectMaxCardinality(0 :r ObjectUnionOf(:Q :P)) :a)",
    );
}
#[test]
#[ignore = "open gap: `≤0 r.C` with a complex qualifier (see the section comment)"]
fn sentinel_complex_qualifier_edge_tautology() {
    let _g = lock();
    assert_inconsistent(U1_EDGE_TAUTOLOGY);
}
#[test]
#[ignore = "open gap: `≤0 r.C` with a complex qualifier (see the section comment)"]
fn sentinel_complex_qualifier_reflexive_union_typed() {
    let _g = lock();
    assert_inconsistent(U2_REFL_UNION_TYPED);
}
#[test]
#[ignore = "open gap: `≤0 r.C` with a complex qualifier (see the section comment)"]
fn sentinel_complex_qualifier_reflexive_tautology() {
    let _g = lock();
    assert_inconsistent(U3_REFL_TAUTOLOGY);
}
#[test]
#[ignore = "open gap: `≤0 r.C` with a complex qualifier (see the section comment)"]
fn sentinel_complex_qualifier_reflexive_intersection_typed() {
    let _g = lock();
    assert_inconsistent(U4_REFL_INTERSECTION_TYPED);
}
#[test]
#[ignore = "open gap: `≤0 r.C` with a complex qualifier (see the section comment)"]
fn sentinel_complex_qualifier_edge_union_typed() {
    let _g = lock();
    assert_inconsistent(U5_EDGE_UNION_TYPED);
}

// ---- the decision is the wedge's, and each flag reverts its fix ----------------

/// The repaired cases are decided by the wedge's own `Unsat`, not by a fall-through:
/// none of them reports a trusted `Sat`. With each fix's flag off, the case it
/// repairs reads `consistent` again on the trusted wedge `Sat`, which pins both the
/// flag and the defect.
#[test]
fn flags_revert_each_fix() {
    let _g = lock();
    for body in [
        N_REFLEXIVE,
        N_SYMMETRIC,
        N_TRANSITIVE,
        A_CHAIN_NEGATIVE,
        M_IF_ALIAS,
        M_FUZZ_IF_ALIAS,
        A_IF_SYMMETRIC,
    ] {
        let _off = SetEnvGuard::set("RUSTDL_WEDGE_ABOX_COVERAGE", "0");
        let (verdict, stats) = consistent(body);
        assert!(
            verdict && stats.relies_on_trusted_consistency,
            "{body}: {stats:?}"
        );
    }
    for body in [M_REFLEXIVE_MAX0, M_REFLEXIVE_INVERSE_MAX0, M_FUZZ_REFLEXIVE] {
        let _off = SetEnvGuard::set("RUSTDL_REFLEXIVE_MAX_ZERO_LOOP", "0");
        let (verdict, stats) = consistent(body);
        assert!(
            verdict && stats.relies_on_trusted_consistency,
            "{body}: {stats:?}"
        );
    }
}
