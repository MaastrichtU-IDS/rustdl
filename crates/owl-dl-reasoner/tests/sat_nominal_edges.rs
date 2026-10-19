//! #279 `RUSTDL_SAT_NOMINAL_EDGES` (default ON): property assertions of individuals
//! used as nominals reach the EL saturator as `NomKey(a) ⊑ ∃S.NomKey(c)`.
//!
//! `X ⊑ ∃R.{a}`, `a S c`, `c : E`, `W ≡ ∃R.∃S.E` entails `X ⊑ W`. The classify wedge
//! can read the edge only through generating clauses, which cost up to 65× on ORE
//! (`RUSTDL_CLASSIFY_NOMINAL_OPA`, default OFF), so `classify` missed it while
//! reporting `incomplete: false`. In the saturator the same facts are polynomial.
//! `HermiT` agrees with every verdict below (2026-10-16).
//!
//! Run: `cargo test -p owl-dl-reasoner --test sat_nominal_edges`

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

const DECLS: &str = "    Declaration(Class(:X)) Declaration(Class(:W)) Declaration(Class(:E))
    Declaration(Class(:F)) Declaration(Class(:B)) Declaration(Class(:C))
    Declaration(ObjectProperty(:R)) Declaration(ObjectProperty(:S))
    Declaration(ObjectProperty(:T)) Declaration(NamedIndividual(:a))
    Declaration(NamedIndividual(:b)) Declaration(NamedIndividual(:c))
    Declaration(Class(:Y)) Declaration(Class(:A)) Declaration(ObjectProperty(:U))
    Declaration(NamedIndividual(:d)) Declaration(NamedIndividual(:e))
    Declaration(NamedIndividual(:f)) Declaration(NamedIndividual(:g))
";

/// The forward reproducer.
const FORWARD: &str = "    SubClassOf(:X ObjectSomeValuesFrom(:R ObjectOneOf(:a)))
    ObjectPropertyAssertion(:S :a :c)
    ClassAssertion(:E :c)
    EquivalentClasses(:W ObjectSomeValuesFrom(:R ObjectSomeValuesFrom(:S :E)))";

/// Classify with `extra` env settings and report `X ⊑ W`.
fn x_is_w(body: &str, extra: &[(&'static str, &str)]) -> bool {
    sub_is_w(body, "X", extra)
}

/// Classify with `extra` env settings and report `sub ⊑ W`.
fn sub_is_w(body: &str, sub: &str, extra: &[(&'static str, &str)]) -> bool {
    let _lock = ENV_MUTEX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _flags: Vec<SetEnvGuard> = extra.iter().map(|(k, v)| SetEnvGuard::set(k, v)).collect();
    let src = format!(
        "Prefix(:=<http://t/>)\nPrefix(owl:=<http://www.w3.org/2002/07/owl#>)\n\
         Ontology(<http://t/x>\n{DECLS}{body}\n)\n"
    );
    let mut reader = Cursor::new(src);
    let (onto, _): (SetOntology<RcStr>, _) =
        read_ofn(&mut reader, ParserConfiguration::default()).expect("parse ofn");
    let c = owl_dl_reasoner::classify(&onto).expect("classify");
    c.is_subclass(&format!("http://t/{sub}"), "http://t/W")
}

// ---------------------------------------------------------------- positives

#[test]
fn a_forward_edge_of_a_nominal_is_followed() {
    assert!(x_is_w(FORWARD, &[]));
}

/// A cycle `a S c`, `c S a` with a deep definition: the wedge's blocking cuts such a
/// cycle, the saturator does not need to unroll it.
#[test]
fn a_cycle_of_edges_is_followed_to_any_depth() {
    let body = "    SubClassOf(:X ObjectSomeValuesFrom(:R ObjectOneOf(:a)))
    ObjectPropertyAssertion(:S :a :c)
    ObjectPropertyAssertion(:S :c :a)
    ClassAssertion(:E :c)
    EquivalentClasses(:W ObjectSomeValuesFrom(:R ObjectSomeValuesFrom(:S
      ObjectSomeValuesFrom(:S ObjectSomeValuesFrom(:S :E)))))";
    assert!(x_is_w(body, &[]));
}

/// An assertion through an inverse role is read forwards: `S⁻(c, a)` is `S(a, c)`.
#[test]
fn an_inverse_role_assertion_is_read_forwards() {
    let body = FORWARD.replace(
        "ObjectPropertyAssertion(:S :a :c)",
        "ObjectPropertyAssertion(ObjectInverseOf(:S) :c :a)",
    );
    assert!(x_is_w(&body, &[]));
}

/// The range of a super-role types the edge's target.
#[test]
fn the_range_of_a_super_role_types_the_target() {
    let body = FORWARD.replace("ClassAssertion(:E :c)", "").replace(
        "ObjectPropertyAssertion(:S :a :c)",
        "ObjectPropertyAssertion(:S :a :c)
    SubObjectPropertyOf(:S :T)
    ObjectPropertyRange(:T :E)",
    );
    assert!(x_is_w(&body, &[]));
}

/// `SameIndividual(a, b)` with the edge on `b`.
#[test]
fn an_edge_of_a_same_individual_is_followed() {
    let body = FORWARD.replace(
        "ObjectPropertyAssertion(:S :a :c)",
        "SameIndividual(:a :b)
    ObjectPropertyAssertion(:S :b :c)",
    );
    assert!(x_is_w(&body, &[]));
}

/// A functional role merges the two asserted successors, which then carry both
/// types (`b = c` is entailed). The control without functionality is below.
const FUNCTIONAL: &str = "    SubClassOf(:X ObjectSomeValuesFrom(:R ObjectOneOf(:a)))
    FunctionalObjectProperty(:S)
    ObjectPropertyAssertion(:S :a :b) ObjectPropertyAssertion(:S :a :c)
    ClassAssertion(:B :b) ClassAssertion(:C :c)
    EquivalentClasses(:W ObjectSomeValuesFrom(:R ObjectSomeValuesFrom(:S
      ObjectIntersectionOf(:B :C))))";

#[test]
fn a_functional_role_merges_the_asserted_successors() {
    assert!(x_is_w(FUNCTIONAL, &[]));
}

/// The range of `S` types the target of an assertion written through `S⁻`.
#[test]
fn a_range_types_the_target_of_an_inverse_assertion() {
    let body = "    SubClassOf(:X ObjectSomeValuesFrom(:R ObjectOneOf(:a)))
    ObjectPropertyAssertion(ObjectInverseOf(:S) :c :a)
    ObjectPropertyRange(:S :E)
    EquivalentClasses(:W ObjectSomeValuesFrom(:R ObjectSomeValuesFrom(:S :E)))";
    assert!(x_is_w(body, &[]));
}

/// A symmetric role's assertion `c S a` is also `a S c`.
#[test]
fn a_symmetric_role_edge_holds_both_ways() {
    let body = FORWARD.replace(
        "ObjectPropertyAssertion(:S :a :c)",
        "SymmetricObjectProperty(:S)
    ObjectPropertyAssertion(:S :c :a)",
    );
    assert!(x_is_w(&body, &[]));
}

/// `⊤ ⊑ E` holds of the edge's target too.
#[test]
fn a_top_subsumer_holds_of_every_reached_individual() {
    let body = FORWARD.replace("ClassAssertion(:E :c)", "SubClassOf(owl:Thing :E)");
    assert!(x_is_w(&body, &[]));
}

// ---------------------------------------------------------------- negatives

/// FP guard: without functionality the two successors need not merge.
#[test]
fn without_functionality_the_successors_stay_apart() {
    let body = FUNCTIONAL.replace("    FunctionalObjectProperty(:S)\n", "");
    assert!(!x_is_w(&body, &[]));
}

/// FP guard: a range of `S` types the TARGET; for `c S a`, that is `a`, not `c`.
#[test]
fn a_range_does_not_type_the_source() {
    let body = "    SubClassOf(:X ObjectSomeValuesFrom(:R ObjectOneOf(:a)))
    ObjectPropertyAssertion(:S :c :a)
    ObjectPropertyRange(:S :E)
    EquivalentClasses(:W ObjectSomeValuesFrom(:R ObjectSomeValuesFrom(:S :E)))";
    assert!(!x_is_w(body, &[]));
}

/// FP guard (polarity): `S ⊑ T⁻` with `Range(T, E)` puts `E` on the SOURCE of an
/// `S`-edge, not its target.
#[test]
fn a_range_through_an_inverse_super_role_is_not_on_the_target() {
    let body = FORWARD.replace(
        "ClassAssertion(:E :c)",
        "SubObjectPropertyOf(:S ObjectInverseOf(:T))
    ObjectPropertyRange(:T :E)",
    );
    assert!(!x_is_w(&body, &[]));
}

/// FP guard: the edge goes the other way (`c S a`), so `a` has no `S`-successor.
#[test]
fn a_reversed_edge_entails_nothing() {
    let body = FORWARD.replace(
        "ObjectPropertyAssertion(:S :a :c)",
        "ObjectPropertyAssertion(:S :c :a)",
    );
    assert!(!x_is_w(&body, &[]));
}

/// FP guard: the edge is on another role.
#[test]
fn an_edge_on_another_role_entails_nothing() {
    let body = FORWARD.replace(
        "ObjectPropertyAssertion(:S :a :c)",
        "ObjectPropertyAssertion(:T :a :c)",
    );
    assert!(!x_is_w(&body, &[]));
}

/// FP guard: the edge's target lacks `E`; a type of another individual is not its.
#[test]
fn a_type_of_another_individual_entails_nothing() {
    let body = FORWARD.replace("ClassAssertion(:E :c)", "ClassAssertion(:E :b)");
    assert!(!x_is_w(&body, &[]));
}

// ------------------------------------------------- the flag is load-bearing

/// `=0` loses the entailment (the wedge's own edge clauses are default OFF).
#[test]
fn flag_off_loses_the_entailment() {
    assert!(!x_is_w(FORWARD, &[("RUSTDL_SAT_NOMINAL_EDGES", "0")]));
}

// ------------------------------------- reverse facts and forced equalities

/// A declared inverse: `c S a` with `Inverse(S, T)` is `a T c`.
#[test]
fn a_declared_inverse_reads_the_edge_backwards() {
    let body = "    SubClassOf(:X ObjectSomeValuesFrom(:R ObjectOneOf(:a)))
    ObjectPropertyAssertion(:S :c :a) InverseObjectProperties(:S :T) ClassAssertion(:E :c)
    EquivalentClasses(:W ObjectSomeValuesFrom(:R ObjectSomeValuesFrom(:T :E)))";
    assert!(x_is_w(body, &[]));
    // FP guard: `a S c` gives `c T a`, not `a T c`.
    let body = body.replace(
        "ObjectPropertyAssertion(:S :c :a)",
        "ObjectPropertyAssertion(:S :a :c)",
    );
    assert!(!x_is_w(&body, &[]));
}

/// `S ⊑ T⁻` puts `T`'s DOMAIN on the target of an `S`-edge.
#[test]
fn a_domain_through_an_inverse_super_role_types_the_target() {
    let body = FORWARD.replace(
        "ClassAssertion(:E :c)",
        "SubObjectPropertyOf(:S ObjectInverseOf(:T))
    ObjectPropertyDomain(:T :E)",
    );
    assert!(x_is_w(&body, &[]));
}

/// `InverseFunctional(S)`: `a S c`, `b S c` give `a = b`, so `a` has `b`'s types.
#[test]
fn an_inverse_functional_role_merges_the_sources() {
    let body = "    SubClassOf(:X ObjectSomeValuesFrom(:R ObjectOneOf(:a)))
    InverseFunctionalObjectProperty(:S)
    ObjectPropertyAssertion(:S :a :c) ObjectPropertyAssertion(:S :b :c)
    ClassAssertion(:A :b)
    EquivalentClasses(:W ObjectSomeValuesFrom(:R :A))";
    assert!(x_is_w(body, &[]));
    let body = body.replace("    InverseFunctionalObjectProperty(:S)\n", "");
    assert!(!x_is_w(&body, &[]));
}

/// A functional merge puts the types on the nominal itself: `a S b`, `a S c`,
/// `Functional(S)`, `b : B` give `c : B`, so `Y ⊑ ∃R.{c}` is `⊑ ∃R.B`.
#[test]
fn a_functional_merge_types_the_merged_nominal() {
    let body = "    FunctionalObjectProperty(:S)
    ObjectPropertyAssertion(:S :a :b) ObjectPropertyAssertion(:S :a :c)
    ClassAssertion(:B :b)
    SubClassOf(:Y ObjectSomeValuesFrom(:R ObjectOneOf(:c)))
    EquivalentClasses(:W ObjectSomeValuesFrom(:R :B))";
    assert!(sub_is_w(body, "Y", &[]));
}

/// A functional SUPER-role merges successors on two different sub-roles.
#[test]
fn a_functional_super_role_merges_across_sub_roles() {
    let body =
        "    SubObjectPropertyOf(:S :T) SubObjectPropertyOf(:U :T) FunctionalObjectProperty(:T)
    ObjectPropertyAssertion(:S :a :b) ObjectPropertyAssertion(:U :a :c) ClassAssertion(:E :b)
    SubClassOf(:Y ObjectSomeValuesFrom(:R ObjectOneOf(:c)))
    EquivalentClasses(:W ObjectSomeValuesFrom(:R :E))";
    assert!(sub_is_w(body, "Y", &[]));
}

/// `S ⊑ T⁻` with `Functional(T)` makes `S` inverse-functional: `b S a`, `c S a`
/// give `b = c`. Reversed edges (`a S b`, `a S c`) force nothing.
#[test]
fn a_functional_inverse_super_role_merges_the_sources() {
    let body = "    SubObjectPropertyOf(:S ObjectInverseOf(:T)) FunctionalObjectProperty(:T)
    ObjectPropertyAssertion(:S :b :a) ObjectPropertyAssertion(:S :c :a) ClassAssertion(:E :b)
    SubClassOf(:Y ObjectSomeValuesFrom(:R ObjectOneOf(:c)))
    EquivalentClasses(:W ObjectSomeValuesFrom(:R :E))";
    assert!(sub_is_w(body, "Y", &[]));
    let body = body
        .replace(
            "ObjectPropertyAssertion(:S :b :a)",
            "ObjectPropertyAssertion(:S :a :b)",
        )
        .replace(
            "ObjectPropertyAssertion(:S :c :a)",
            "ObjectPropertyAssertion(:S :a :c)",
        );
    assert!(!sub_is_w(&body, "Y", &[]));
}

/// Merges feed each other: `Functional(S)` gives `b = c`, and then
/// `InverseFunctional(U)` with `d U b`, `e U c` gives `d = e`.
#[test]
fn forced_equalities_are_closed_under_each_other() {
    let body = "    FunctionalObjectProperty(:S) InverseFunctionalObjectProperty(:U)
    ObjectPropertyAssertion(:S :a :b) ObjectPropertyAssertion(:S :a :c)
    ObjectPropertyAssertion(:U :d :b) ObjectPropertyAssertion(:U :e :c)
    ClassAssertion(:E :d)
    SubClassOf(:Y ObjectSomeValuesFrom(:R ObjectOneOf(:e)))
    EquivalentClasses(:W ObjectSomeValuesFrom(:R :E))";
    assert!(sub_is_w(body, "Y", &[]));
    // Without the functional S, b and c stay apart, and so do d and e.
    let body = body.replace("FunctionalObjectProperty(:S) ", "");
    assert!(!sub_is_w(&body, "Y", &[]));
}

/// Three merges in a chain, each enabled by the previous: `b = c` (functional
/// `S`), then `d = e` (inverse-functional `U`), then `f = g` (`S` again). Whichever
/// order the two roles are processed in, one pass cannot finish it.
#[test]
fn a_chain_of_forced_equalities_needs_a_fixpoint() {
    let body = "    FunctionalObjectProperty(:S) InverseFunctionalObjectProperty(:U)
    ObjectPropertyAssertion(:S :a :b) ObjectPropertyAssertion(:S :a :c)
    ObjectPropertyAssertion(:U :d :b) ObjectPropertyAssertion(:U :e :c)
    ObjectPropertyAssertion(:S :d :f) ObjectPropertyAssertion(:S :e :g)
    ClassAssertion(:E :f)
    SubClassOf(:Y ObjectSomeValuesFrom(:R ObjectOneOf(:g)))
    EquivalentClasses(:W ObjectSomeValuesFrom(:R :E))";
    assert!(sub_is_w(body, "Y", &[]));
}

/// KNOWN MISS, deliberate: no reverse fact for a role under a transitive role or a
/// role-chain leg, because it lets the saturator materialise relation closures over
/// every reached individual (family.ofn: 10 s → 238 s). Entailed, but not derived
/// here. FLIP this if that cost is ever solved.
#[test]
fn a_declared_inverse_of_a_transitive_role_is_not_followed() {
    let body = "    SubClassOf(:X ObjectSomeValuesFrom(:R ObjectOneOf(:a)))
    ObjectPropertyAssertion(:S :c :a) InverseObjectProperties(:S :T) ClassAssertion(:E :c)
    TransitiveObjectProperty(:T)
    EquivalentClasses(:W ObjectSomeValuesFrom(:R ObjectSomeValuesFrom(:T :E)))";
    assert!(!x_is_w(body, &[]));
}

/// A symmetric role that is also transitive keeps its own reverse edge: the
/// composition skip exempts it (it feeds the same role the forward edge feeds).
#[test]
fn a_symmetric_transitive_role_keeps_its_reverse_edge() {
    let body = "    SubClassOf(:X ObjectSomeValuesFrom(:R ObjectOneOf(:a)))
    SymmetricObjectProperty(:S) TransitiveObjectProperty(:S)
    ObjectPropertyAssertion(:S :c :a) ClassAssertion(:E :c)
    EquivalentClasses(:W ObjectSomeValuesFrom(:R ObjectSomeValuesFrom(:S :E)))";
    assert!(x_is_w(body, &[]));
}

/// A symmetric functional role is read both ways for equalities: `a S b`, `c S b`
/// are also `b S a`, `b S c`, so `a = c`.
#[test]
fn a_symmetric_functional_role_merges_both_ways() {
    let body = "    SymmetricObjectProperty(:S) FunctionalObjectProperty(:S)
    ObjectPropertyAssertion(:S :a :b) ObjectPropertyAssertion(:S :c :b) ClassAssertion(:A :a)
    SubClassOf(:Y ObjectSomeValuesFrom(:R ObjectOneOf(:c)))
    EquivalentClasses(:W ObjectSomeValuesFrom(:R :A))";
    assert!(sub_is_w(body, "Y", &[]));
}

/// An asserted `SameIndividual` feeds the forced equalities: `a = d`, `a S b`,
/// `d S c`, `Functional(S)` give `b = c`.
#[test]
fn an_asserted_same_individual_feeds_functional_merges() {
    let body = "    FunctionalObjectProperty(:S) SameIndividual(:a :d)
    ObjectPropertyAssertion(:S :a :b) ObjectPropertyAssertion(:S :d :c) ClassAssertion(:B :b)
    SubClassOf(:Y ObjectSomeValuesFrom(:R ObjectOneOf(:c)))
    EquivalentClasses(:W ObjectSomeValuesFrom(:R :B))";
    assert!(sub_is_w(body, "Y", &[]));
}
