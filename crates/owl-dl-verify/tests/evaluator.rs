//! Tests for `eval::eval_concept`, driven by a hand-built `Interpretation`
//! stub rather than `FiniteModel`. That keeps these tests asserting
//! `eval_concept`'s own behaviour without depending on the builder's
//! correctness — the two are meant to be checked independently of each
//! other.

use std::collections::HashSet;

use owl_dl_core::{
    Axiom, ClassId, ConceptId, ConceptPool, IndividualId, InternalOntology, Role, RoleId,
    SubRolePath,
};
use owl_dl_verify::eval::{AxiomVerdict, Judgement, check_axiom, eval_concept};
use owl_dl_verify::{Bounds, Element, Interpretation, UnresolvedReason, Verdict, verify};

mod common;

/// A minimal, fully-controlled `Interpretation`: explicit membership and
/// edge sets, nothing derived or inferred.
struct StubModel {
    domain: Vec<Element>,
    membership: HashSet<(Element, ClassId)>,
    edges: Vec<(Element, RoleId, Element)>,
}

impl StubModel {
    fn new(size: u32) -> Self {
        Self {
            domain: (0..size).map(Element::new).collect(),
            membership: HashSet::new(),
            edges: Vec::new(),
        }
    }

    fn assert_member(&mut self, e: Element, c: ClassId) {
        self.membership.insert((e, c));
    }

    fn assert_edge(&mut self, from: Element, r: RoleId, to: Element) {
        self.edges.push((from, r, to));
    }
}

impl Interpretation for StubModel {
    fn domain_size(&self) -> usize {
        self.domain.len()
    }

    fn elements(&self) -> impl Iterator<Item = Element> + '_ {
        self.domain.iter().copied()
    }

    fn in_concept(&self, e: Element, c: ClassId) -> bool {
        self.membership.contains(&(e, c))
    }

    fn successors(&self, e: Element, r: RoleId) -> Vec<Element> {
        self.edges
            .iter()
            .filter(|(from, role, _)| *from == e && *role == r)
            .map(|(_, _, to)| *to)
            .collect()
    }

    fn has_edge(&self, from: Element, r: RoleId, to: Element) -> bool {
        self.edges.contains(&(from, r, to))
    }

    fn edges(&self, r: RoleId) -> Vec<(Element, Element)> {
        self.edges
            .iter()
            .filter(|(_, role, _)| *role == r)
            .map(|(from, _, to)| (*from, *to))
            .collect()
    }

    fn num_roles(&self) -> usize {
        self.edges
            .iter()
            .map(|(_, role, _)| role.index())
            .collect::<HashSet<_>>()
            .len()
    }
}

#[test]
fn top_is_true_bot_is_false_everywhere() {
    let mut pool = ConceptPool::new();
    let top = pool.top();
    let bot = pool.bot();
    let model = StubModel::new(1);
    let e = Element::new(0);

    assert_eq!(eval_concept(&pool, &model, e, top), Judgement::True);
    assert_eq!(eval_concept(&pool, &model, e, bot), Judgement::False);
}

#[test]
fn unhandled_concept_forms_are_unresolved_not_false() {
    // All(r, X) is out of fragment: it MUST NOT silently evaluate to true or
    // false, because either would let a stub pass as a real check.
    let mut pool = ConceptPool::new();
    let r = RoleId::new(0);
    let x = pool.top();
    let all_r_x = pool.all(owl_dl_core::Role::named(r), x);

    let model = StubModel::new(1);
    let e = Element::new(0);

    assert_eq!(
        eval_concept(&pool, &model, e, all_r_x),
        Judgement::Unresolved("All")
    );
}

#[test]
fn and_short_circuits_to_false_even_with_an_unresolved_operand() {
    // A ⊓ (∀r.X): A is false at e, ∀r.X is Unresolved. False must win.
    let mut pool = ConceptPool::new();
    let a = pool.atomic(ClassId::new(0));
    let top = pool.top();
    let all_r_x = pool.all(owl_dl_core::Role::named(RoleId::new(0)), top);
    let conj = pool.and([a, all_r_x]);

    let model = StubModel::new(1);
    let e = Element::new(0);
    // `a` is never asserted a member, so it evaluates False.

    assert_eq!(eval_concept(&pool, &model, e, conj), Judgement::False);
}

#[test]
fn and_is_unresolved_when_no_operand_is_false_but_one_is_unresolved() {
    // A ⊓ (∀r.X): A is True (asserted member), ∀r.X is Unresolved. Must be
    // Unresolved, not True.
    //
    // NOTE: using `pool.top()` as the True operand here is a trap:
    // `ConceptPool::and` drops `Top` as the identity element and, once only
    // one non-Top operand remains, returns that operand UNWRAPPED rather than
    // an `And` node — so `pool.and([top, all_r_x])` collapses to `all_r_x`
    // itself and the test would silently stop exercising the `And` arm at
    // all (it would re-test the trivial `All => Unresolved` arm instead).
    // Use a second genuine, non-identity, True-yielding operand instead, and
    // assert the pool actually produced an `And` node before trusting the
    // rest of the test.
    let mut pool = ConceptPool::new();
    let a = pool.atomic(ClassId::new(0));
    let top = pool.top();
    let all_r_x = pool.all(owl_dl_core::Role::named(RoleId::new(0)), top);
    let conj = pool.and([a, all_r_x]);
    assert!(
        matches!(pool.get(conj), owl_dl_core::ConceptExpr::And(_)),
        "pool.and collapsed the node; this test would not exercise the And arm"
    );

    let mut model = StubModel::new(1);
    let e = Element::new(0);
    model.assert_member(e, ClassId::new(0));

    assert_eq!(
        eval_concept(&pool, &model, e, conj),
        Judgement::Unresolved("All")
    );
}

#[test]
fn some_short_circuits_to_true_on_a_true_witness_even_with_another_unresolved_successor() {
    // e --r--> t1 (member of A), e --r--> t2 (unjudgeable via All(s, X)).
    // ∃r.A must be True because t1 is a confirmed witness, regardless of t2.
    let mut pool = ConceptPool::new();
    let a = ClassId::new(0);
    let a_concept = pool.atomic(a);
    let r = RoleId::new(0);
    let some_r_a = pool.some(owl_dl_core::Role::named(r), a_concept);

    let mut model = StubModel::new(3);
    let e = Element::new(0);
    let t1 = Element::new(1);
    let t2 = Element::new(2);
    model.assert_member(t1, a);
    model.assert_edge(e, r, t1);
    model.assert_edge(e, r, t2);

    assert_eq!(eval_concept(&pool, &model, e, some_r_a), Judgement::True);
}

#[test]
fn some_is_unresolved_when_no_witness_is_true_but_one_is_unresolved() {
    // e --r--> t (unjudgeable via All(s, X), no other witness).
    // ∃r.(∀s.X) must be Unresolved, not False: absence of a confirmed
    // witness is not a confirmed absence of one.
    let mut pool = ConceptPool::new();
    let r = RoleId::new(0);
    let s = RoleId::new(1);
    let top = pool.top();
    let all_s_top = pool.all(owl_dl_core::Role::named(s), top);
    let some_r_all = pool.some(owl_dl_core::Role::named(r), all_s_top);

    let mut model = StubModel::new(2);
    let e = Element::new(0);
    let t = Element::new(1);
    model.assert_edge(e, r, t);

    assert_eq!(
        eval_concept(&pool, &model, e, some_r_all),
        Judgement::Unresolved("All")
    );
}

#[test]
fn some_is_false_when_there_are_no_successors_at_all() {
    let mut pool = ConceptPool::new();
    let r = RoleId::new(0);
    let top = pool.top();
    let some_r_top = pool.some(owl_dl_core::Role::named(r), top);

    let model = StubModel::new(1);
    let e = Element::new(0);

    assert_eq!(eval_concept(&pool, &model, e, some_r_top), Judgement::False);
}

/// Confirms the extracted `common::load` helper is reachable from this test
/// binary too (it is shared with `tests/model.rs`, not re-copied), and that
/// `eval_concept` agrees with a real ontology's told-atomic membership.
#[test]
fn eval_concept_agrees_with_a_real_ontologys_told_atomic_class() {
    const ONTO: &str = r"Prefix(:=<http://ex.org/>)
Ontology(<http://ex.org/t>
Declaration(Class(:A))
Declaration(Class(:B))
SubClassOf(:A :B)
)
";
    let internal = common::load(ONTO);
    let (subs, facts, _) = owl_dl_saturation::saturate_with_exists_facts(&internal);
    let model = owl_dl_verify::model::FiniteModel::seed(&internal, &subs, &facts);

    let a = internal.vocabulary.class_id("http://ex.org/A").expect("A");
    let b = internal.vocabulary.class_id("http://ex.org/B").expect("B");
    let e = model.element_of_class(a).expect("A is satisfiable");

    let mut pool = ConceptPool::new();
    let a_concept = pool.atomic(a);
    let b_concept = pool.atomic(b);

    assert_eq!(eval_concept(&pool, &model, e, a_concept), Judgement::True);
    assert_eq!(
        eval_concept(&pool, &model, e, b_concept),
        Judgement::True,
        "A ⊑ B, so B must be told-derivable in the label FiniteModel seeded"
    );
}

// --- check_axiom: the 8 class-shaped variants + a 5-variant sabotage matrix ---
//
// Everything below drives `FiniteModel` via `build_model`, not the hand-built
// `StubModel` above: these tests are about `check_axiom` finding (or missing)
// a violation in a REAL model, not about `eval_concept`'s own three-valued
// logic in isolation (that is what the tests above already cover).

/// The UNIQUE axiom in `internal.axioms` matching `pred`, or panics — a
/// fixture missing the axiom it was written to exercise, or one asserting
/// TWO axioms that both match, is a test-authoring bug, not something to
/// silently tolerate. Task 8's version took the first match with no
/// uniqueness check; that was safe only because every fixture so far
/// happened to have exactly one match per predicate, which uniqueness now
/// makes an enforced property of every fixture rather than an unstated one.
fn axiom_index(internal: &InternalOntology, pred: impl Fn(&Axiom) -> bool) -> usize {
    let matches: Vec<usize> = internal
        .axioms
        .iter()
        .enumerate()
        .filter_map(|(i, ax)| pred(ax).then_some(i))
        .collect();
    assert_eq!(
        matches.len(),
        1,
        "fixture must contain EXACTLY ONE axiom matching this predicate (found {}); \
         a first-match lookup would silently pick the wrong one",
        matches.len()
    );
    matches[0]
}

const DECLARATIONS_FIXTURE: &str = r"Prefix(:=<http://ex.org/>)
Ontology(<http://ex.org/decl>
Declaration(Class(:A))
Declaration(ObjectProperty(:p))
Declaration(NamedIndividual(:x))
)
";

const SUBCLASS_FIXTURE: &str = r"Prefix(:=<http://ex.org/>)
Ontology(<http://ex.org/sc>
Declaration(Class(:A)) Declaration(Class(:B))
SubClassOf(:A :B)
)
";

const EQUIV_FIXTURE: &str = r"Prefix(:=<http://ex.org/>)
Ontology(<http://ex.org/eq>
Declaration(Class(:A)) Declaration(Class(:B))
EquivalentClasses(:A :B)
)
";

const DISJOINT_FIXTURE: &str = r"Prefix(:=<http://ex.org/>)
Ontology(<http://ex.org/dj>
Declaration(Class(:A)) Declaration(Class(:B))
DisjointClasses(:A :B)
)
";

// `SubClassOf(:C :D)` makes the domain constraint hold BY CONSTRUCTION,
// decoupling this fixture from whether the saturator's own domain-propagation
// rule fires — that rule's correctness is not this test's concern, only
// `check_axiom`'s independent re-check of it is.
const DOMAIN_FIXTURE: &str = r"Prefix(:=<http://ex.org/>)
Ontology(<http://ex.org/dom>
Declaration(Class(:C)) Declaration(Class(:D)) Declaration(Class(:E))
Declaration(ObjectProperty(:p))
ObjectPropertyDomain(:p :D)
SubClassOf(:C :D)
SubClassOf(:C ObjectSomeValuesFrom(:p :E))
)
";

// E and F are deliberately UNRELATED: the range-fold + injection machinery
// (`build_model` §5) must close F into the witness's label on its own.
const RANGE_FIXTURE: &str = r"Prefix(:=<http://ex.org/>)
Ontology(<http://ex.org/rng>
Declaration(Class(:C)) Declaration(Class(:E)) Declaration(Class(:F))
Declaration(ObjectProperty(:p))
ObjectPropertyRange(:p :F)
SubClassOf(:C ObjectSomeValuesFrom(:p :E))
)
";

#[test]
fn declare_class_axiom_is_vacuously_holds() {
    let internal = common::load(DECLARATIONS_FIXTURE);
    let (m, _) = owl_dl_verify::build_model(&internal, &Bounds::default()).expect("builds");
    let idx = axiom_index(&internal, |ax| matches!(ax, Axiom::DeclareClass(_)));
    assert!(matches!(
        check_axiom(&internal.concepts, &m, idx, &internal.axioms[idx]),
        AxiomVerdict::Holds
    ));
}

#[test]
fn declare_object_property_axiom_is_vacuously_holds() {
    let internal = common::load(DECLARATIONS_FIXTURE);
    let (m, _) = owl_dl_verify::build_model(&internal, &Bounds::default()).expect("builds");
    let idx = axiom_index(&internal, |ax| {
        matches!(ax, Axiom::DeclareObjectProperty(_))
    });
    assert!(matches!(
        check_axiom(&internal.concepts, &m, idx, &internal.axioms[idx]),
        AxiomVerdict::Holds
    ));
}

#[test]
fn declare_named_individual_axiom_is_vacuously_holds() {
    let internal = common::load(DECLARATIONS_FIXTURE);
    let (m, _) = owl_dl_verify::build_model(&internal, &Bounds::default()).expect("builds");
    let idx = axiom_index(&internal, |ax| {
        matches!(ax, Axiom::DeclareNamedIndividual(_))
    });
    assert!(matches!(
        check_axiom(&internal.concepts, &m, idx, &internal.axioms[idx]),
        AxiomVerdict::Holds
    ));
}

#[test]
fn subclassof_holds_on_a_healthy_ontology() {
    let internal = common::load(SUBCLASS_FIXTURE);
    let (m, _) = owl_dl_verify::build_model(&internal, &Bounds::default()).expect("builds");
    let idx = axiom_index(&internal, |ax| matches!(ax, Axiom::SubClassOf { .. }));
    assert!(matches!(
        check_axiom(&internal.concepts, &m, idx, &internal.axioms[idx]),
        AxiomVerdict::Holds
    ));
}

#[test]
fn equivalent_classes_holds_on_a_healthy_ontology() {
    let internal = common::load(EQUIV_FIXTURE);
    let (m, _) = owl_dl_verify::build_model(&internal, &Bounds::default()).expect("builds");
    let idx = axiom_index(&internal, |ax| matches!(ax, Axiom::EquivalentClasses(_)));
    assert!(matches!(
        check_axiom(&internal.concepts, &m, idx, &internal.axioms[idx]),
        AxiomVerdict::Holds
    ));
}

#[test]
fn disjoint_classes_holds_on_a_healthy_ontology() {
    let internal = common::load(DISJOINT_FIXTURE);
    let (m, _) = owl_dl_verify::build_model(&internal, &Bounds::default()).expect("builds");
    let idx = axiom_index(&internal, |ax| matches!(ax, Axiom::DisjointClasses(_)));
    assert!(matches!(
        check_axiom(&internal.concepts, &m, idx, &internal.axioms[idx]),
        AxiomVerdict::Holds
    ));
}

#[test]
fn object_property_domain_holds_on_a_healthy_ontology() {
    let internal = common::load(DOMAIN_FIXTURE);
    let (m, _) = owl_dl_verify::build_model(&internal, &Bounds::default()).expect("builds");
    let p = internal.vocabulary.role_id("http://ex.org/p").expect("p");
    assert!(
        !m.edges(p).is_empty(),
        "the fixture must actually produce a p-edge, or this Holds is vacuous truth"
    );
    let idx = axiom_index(&internal, |ax| {
        matches!(ax, Axiom::ObjectPropertyDomain { .. })
    });
    assert!(matches!(
        check_axiom(&internal.concepts, &m, idx, &internal.axioms[idx]),
        AxiomVerdict::Holds
    ));
}

#[test]
fn object_property_range_holds_on_a_healthy_ontology() {
    let internal = common::load(RANGE_FIXTURE);
    let (m, _) = owl_dl_verify::build_model(&internal, &Bounds::default()).expect("builds");
    let p = internal.vocabulary.role_id("http://ex.org/p").expect("p");
    assert!(
        !m.edges(p).is_empty(),
        "the fixture must actually produce a p-edge, or this Holds is vacuous truth"
    );
    let idx = axiom_index(&internal, |ax| {
        matches!(ax, Axiom::ObjectPropertyRange { .. })
    });
    assert!(matches!(
        check_axiom(&internal.concepts, &m, idx, &internal.axioms[idx]),
        AxiomVerdict::Holds
    ));
}

#[test]
fn sabotage_subclassof_a_deleted_label_entry_must_be_caught_with_index_and_witness() {
    let internal = common::load(SUBCLASS_FIXTURE);
    let (mut m, _) = owl_dl_verify::build_model(&internal, &Bounds::default()).expect("builds");
    let idx = axiom_index(&internal, |ax| matches!(ax, Axiom::SubClassOf { .. }));
    assert!(matches!(
        check_axiom(&internal.concepts, &m, idx, &internal.axioms[idx]),
        AxiomVerdict::Holds
    ));

    let a = internal.vocabulary.class_id("http://ex.org/A").expect("A");
    let b = internal.vocabulary.class_id("http://ex.org/B").expect("B");
    let elem_a = m.element_of_class(a).expect("A is satisfiable");
    m.test_only_remove_from_label(elem_a, b);

    match check_axiom(&internal.concepts, &m, idx, &internal.axioms[idx]) {
        AxiomVerdict::Fails { witness, .. } => {
            assert_eq!(
                witness,
                vec![elem_a],
                "witness must be pinned to A's element"
            );
        }
        other => panic!("mutation must be caught, got {other:?}"),
    }
}

#[test]
fn sabotage_equivalent_classes_a_deleted_label_entry_must_be_caught_with_index_and_witness() {
    let internal = common::load(EQUIV_FIXTURE);
    let (mut m, _) = owl_dl_verify::build_model(&internal, &Bounds::default()).expect("builds");
    let idx = axiom_index(&internal, |ax| matches!(ax, Axiom::EquivalentClasses(_)));
    assert!(matches!(
        check_axiom(&internal.concepts, &m, idx, &internal.axioms[idx]),
        AxiomVerdict::Holds
    ));

    let a = internal.vocabulary.class_id("http://ex.org/A").expect("A");
    let b = internal.vocabulary.class_id("http://ex.org/B").expect("B");
    // A and B are equivalent, so they share ONE element.
    let elem = m.element_of_class(a).expect("A is satisfiable");
    assert_eq!(elem, m.element_of_class(b).expect("B is satisfiable"));
    m.test_only_remove_from_label(elem, b);

    match check_axiom(&internal.concepts, &m, idx, &internal.axioms[idx]) {
        AxiomVerdict::Fails { witness, .. } => {
            assert_eq!(
                witness,
                vec![elem],
                "witness must be pinned to the shared element"
            );
        }
        other => panic!("mutation must be caught, got {other:?}"),
    }
}

#[test]
fn sabotage_disjoint_classes_two_true_members_must_be_caught_with_index_and_witness() {
    let internal = common::load(DISJOINT_FIXTURE);
    let (mut m, _) = owl_dl_verify::build_model(&internal, &Bounds::default()).expect("builds");
    let idx = axiom_index(&internal, |ax| matches!(ax, Axiom::DisjointClasses(_)));
    assert!(matches!(
        check_axiom(&internal.concepts, &m, idx, &internal.axioms[idx]),
        AxiomVerdict::Holds
    ));

    // `test_only_remove_from_label` can only turn a `True` into a `False` —
    // it is monotonically truth-DECREASING, so no removal can ever
    // manufacture the two-members-true violation `DisjointClasses` forbids.
    // `intern` is already unconditionally public production API (the
    // builder itself uses it throughout), so it is used here to add the one
    // element neither the healthy ontology nor `test_only_remove_from_label`
    // can produce, without introducing any new test-only surface.
    let a = internal.vocabulary.class_id("http://ex.org/A").expect("A");
    let b = internal.vocabulary.class_id("http://ex.org/B").expect("B");
    let mut label = vec![a, b];
    label.sort_unstable_by_key(|c| c.index());
    let culprit = m.intern(label);

    match check_axiom(&internal.concepts, &m, idx, &internal.axioms[idx]) {
        AxiomVerdict::Fails { witness, .. } => {
            assert_eq!(
                witness,
                vec![culprit],
                "witness must be pinned to the manufactured element"
            );
        }
        other => panic!("mutation must be caught, got {other:?}"),
    }
}

#[test]
fn sabotage_domain_a_deleted_label_entry_must_be_caught_with_index_and_witness() {
    let internal = common::load(DOMAIN_FIXTURE);
    let (mut m, _) = owl_dl_verify::build_model(&internal, &Bounds::default()).expect("builds");
    let idx = axiom_index(&internal, |ax| {
        matches!(ax, Axiom::ObjectPropertyDomain { .. })
    });
    assert!(matches!(
        check_axiom(&internal.concepts, &m, idx, &internal.axioms[idx]),
        AxiomVerdict::Holds
    ));

    let c = internal.vocabulary.class_id("http://ex.org/C").expect("C");
    let d = internal.vocabulary.class_id("http://ex.org/D").expect("D");
    let p = internal.vocabulary.role_id("http://ex.org/p").expect("p");
    let elem_c = m.element_of_class(c).expect("C is satisfiable");
    assert!(
        !m.successors(elem_c, p).is_empty(),
        "C must have gained a p-edge from its own axiom"
    );
    m.test_only_remove_from_label(elem_c, d);

    match check_axiom(&internal.concepts, &m, idx, &internal.axioms[idx]) {
        AxiomVerdict::Fails { witness, .. } => {
            assert_eq!(
                witness,
                vec![elem_c],
                "witness must be pinned to the edge SOURCE"
            );
        }
        other => panic!("mutation must be caught, got {other:?}"),
    }
}

#[test]
fn sabotage_range_a_deleted_label_entry_must_be_caught_with_index_and_witness() {
    let internal = common::load(RANGE_FIXTURE);
    let (mut m, _) = owl_dl_verify::build_model(&internal, &Bounds::default()).expect("builds");
    let idx = axiom_index(&internal, |ax| {
        matches!(ax, Axiom::ObjectPropertyRange { .. })
    });
    assert!(matches!(
        check_axiom(&internal.concepts, &m, idx, &internal.axioms[idx]),
        AxiomVerdict::Holds
    ));

    let c = internal.vocabulary.class_id("http://ex.org/C").expect("C");
    let f = internal.vocabulary.class_id("http://ex.org/F").expect("F");
    let p = internal.vocabulary.role_id("http://ex.org/p").expect("p");
    let elem_c = m.element_of_class(c).expect("C is satisfiable");
    let succs = m.successors(elem_c, p);
    assert!(!succs.is_empty(), "the p-edge must exist");
    let target = succs[0];
    assert!(
        m.in_concept(target, f),
        "Range(p,F) must be closed into the witness's label before the mutation"
    );
    m.test_only_remove_from_label(target, f);

    match check_axiom(&internal.concepts, &m, idx, &internal.axioms[idx]) {
        AxiomVerdict::Fails { witness, .. } => {
            assert_eq!(
                witness,
                vec![target],
                "witness must be pinned to the edge TARGET"
            );
        }
        other => panic!("mutation must be caught, got {other:?}"),
    }
}

// --- check_axiom: Unresolved PROPAGATION at the check_axiom level ---
//
// Task 8's review found nothing pinning this: only eval_concept-level
// Unresolved was tested. Both behaviours below are already correct on
// `main` — these are regression tests, not new logic.

#[test]
fn subclassof_with_an_all_values_from_superclass_is_unresolved_not_holds() {
    const ONTO: &str = r"Prefix(:=<http://ex.org/>)
Ontology(<http://ex.org/allvf>
Declaration(Class(:A)) Declaration(Class(:B))
Declaration(ObjectProperty(:r))
SubClassOf(:A ObjectAllValuesFrom(:r :B))
)
";
    let internal = common::load(ONTO);
    let (m, _) = owl_dl_verify::build_model(&internal, &Bounds::default()).expect("builds");
    let idx = axiom_index(&internal, |ax| matches!(ax, Axiom::SubClassOf { .. }));
    match check_axiom(&internal.concepts, &m, idx, &internal.axioms[idx]) {
        AxiomVerdict::Unresolved(UnresolvedReason::UnhandledConcept { variant, .. }) => {
            assert_eq!(variant, "All");
        }
        other => panic!("expected Unresolved(UnhandledConcept), got {other:?}"),
    }
}

#[test]
fn object_property_domain_on_an_inverse_role_is_unresolved_not_holds() {
    const ONTO: &str = r"Prefix(:=<http://ex.org/>)
Ontology(<http://ex.org/domiv>
Declaration(Class(:D))
Declaration(ObjectProperty(:p))
ObjectPropertyDomain(ObjectInverseOf(:p) :D)
)
";
    let internal = common::load(ONTO);
    let (m, _) = owl_dl_verify::build_model(&internal, &Bounds::default()).expect("builds");
    let idx = axiom_index(&internal, |ax| {
        matches!(ax, Axiom::ObjectPropertyDomain { .. })
    });
    assert!(matches!(
        check_axiom(&internal.concepts, &m, idx, &internal.axioms[idx]),
        AxiomVerdict::Unresolved(UnresolvedReason::UnhandledAxiom { .. })
    ));
}

// --- check_axiom: the 5 role-shaped variants ---

const SUBROLE_FIXTURE: &str = r"Prefix(:=<http://ex.org/>)
Ontology(<http://ex.org/subrole>
Declaration(Class(:C)) Declaration(Class(:D))
Declaration(ObjectProperty(:p)) Declaration(ObjectProperty(:q)) Declaration(ObjectProperty(:z))
SubObjectPropertyOf(:p :q)
SubClassOf(:C ObjectSomeValuesFrom(:p :D))
)
";

#[test]
fn subobjectpropertyof_role_holds_on_a_healthy_ontology() {
    let internal = common::load(SUBROLE_FIXTURE);
    let (m, _) = owl_dl_verify::build_model(&internal, &Bounds::default()).expect("builds");
    let p = internal.vocabulary.role_id("http://ex.org/p").expect("p");
    assert!(
        !m.edges(p).is_empty(),
        "fixture must actually produce a p-edge, or Holds is vacuous truth"
    );
    let idx = axiom_index(&internal, |ax| {
        matches!(
            ax,
            Axiom::SubObjectPropertyOf {
                sub: SubRolePath::Role(_),
                ..
            }
        )
    });
    assert!(matches!(
        check_axiom(&internal.concepts, &m, idx, &internal.axioms[idx]),
        AxiomVerdict::Holds
    ));
}

/// Edge deletion cannot exercise this arm's `Fails` branch — see `eval.rs`'s
/// doc on `check_axiom` for why `SubObjectPropertyOf(Role)` is structurally
/// un-sabotageable that way, and
/// `subobjectpropertyof_role_edge_deletion_is_structurally_a_no_op` below for
/// the direct demonstration. Instead, this proves the SAME check logic
/// genuinely detects a violation by passing `check_axiom` an axiom the
/// model's hierarchy does NOT reflect: `p`'s real edge is not also a
/// `z`-edge (nothing relates `p` and `z` in this ontology), which is exactly
/// the shape of violation the axiom's semantics forbid.
#[test]
fn sabotage_subobjectpropertyof_role_a_mismatched_super_role_must_be_caught_with_index_and_witness()
{
    let internal = common::load(SUBROLE_FIXTURE);
    let (m, _) = owl_dl_verify::build_model(&internal, &Bounds::default()).expect("builds");
    let idx = axiom_index(&internal, |ax| {
        matches!(
            ax,
            Axiom::SubObjectPropertyOf {
                sub: SubRolePath::Role(_),
                ..
            }
        )
    });

    let p = internal.vocabulary.role_id("http://ex.org/p").expect("p");
    let z = internal.vocabulary.role_id("http://ex.org/z").expect("z");
    let edges = m.edges(p);
    assert_eq!(edges.len(), 1, "fixture must have exactly one p-edge");
    let (from, to) = edges[0];

    let mismatched = Axiom::SubObjectPropertyOf {
        sub: SubRolePath::Role(Role::named(p)),
        sup: Role::named(z),
    };
    match check_axiom(&internal.concepts, &m, idx, &mismatched) {
        AxiomVerdict::Fails { witness, .. } => {
            assert_eq!(
                witness,
                vec![from, to],
                "witness must be the mismatched edge's endpoints"
            );
        }
        other => panic!("mismatched axiom must be caught, got {other:?}"),
    }
}

/// Documents (as an executable assertion, not just a comment) the structural
/// fact this arm's doc explains: `has_edge(sup)` already unions in `sub`'s
/// own bucket via the SAME `RoleHierarchy` `build_role_hierarchy` built FROM
/// this exact axiom, so deleting the only `p`-edge removes it from the
/// antecedent (`edges(p)`) too. The check reads `Holds` either way — this
/// arm genuinely CAN be stubbed to `Holds` and an edge-deletion-style
/// sabotage test would not notice.
#[test]
fn subobjectpropertyof_role_edge_deletion_is_structurally_a_no_op() {
    let internal = common::load(SUBROLE_FIXTURE);
    let (mut m, _) = owl_dl_verify::build_model(&internal, &Bounds::default()).expect("builds");
    let idx = axiom_index(&internal, |ax| {
        matches!(
            ax,
            Axiom::SubObjectPropertyOf {
                sub: SubRolePath::Role(_),
                ..
            }
        )
    });
    let p = internal.vocabulary.role_id("http://ex.org/p").expect("p");
    let edges = m.edges(p);
    assert_eq!(edges.len(), 1);
    let (from, to) = edges[0];

    m.test_only_remove_edge(p, from, to);
    assert!(
        m.edges(p).is_empty(),
        "the antecedent vanished along with the edge"
    );
    assert!(
        matches!(
            check_axiom(&internal.concepts, &m, idx, &internal.axioms[idx]),
            AxiomVerdict::Holds
        ),
        "deleting the only p-edge cannot produce a violation: the antecedent is now empty too"
    );
}

const CHAIN_FIXTURE: &str = r"Prefix(:=<http://ex.org/>)
Ontology(<http://ex.org/chain9>
Declaration(Class(:C)) Declaration(Class(:A))
Declaration(ObjectProperty(:t)) Declaration(ObjectProperty(:u)) Declaration(ObjectProperty(:v))
SubObjectPropertyOf(ObjectPropertyChain(:t :u) :v)
SubClassOf(:C ObjectSomeValuesFrom(:t ObjectSomeValuesFrom(:u :A)))
)
";

fn chain_axiom_index(internal: &InternalOntology) -> usize {
    axiom_index(internal, |ax| {
        matches!(
            ax,
            Axiom::SubObjectPropertyOf {
                sub: SubRolePath::Chain(_),
                ..
            }
        )
    })
}

#[test]
fn subobjectpropertyof_chain_holds_on_a_healthy_ontology() {
    let internal = common::load(CHAIN_FIXTURE);
    let (m, _) = owl_dl_verify::build_model(&internal, &Bounds::default()).expect("builds");
    let v = internal.vocabulary.role_id("http://ex.org/v").expect("v");
    assert!(
        !m.edges(v).is_empty(),
        "chain composition must materialise a v-edge, or Holds is vacuous truth"
    );
    let idx = chain_axiom_index(&internal);
    assert!(matches!(
        check_axiom(&internal.concepts, &m, idx, &internal.axioms[idx]),
        AxiomVerdict::Holds
    ));
}

#[test]
#[allow(clippy::many_single_char_names)] // t, u, v, w, z mirror the fixture's own role/element names
fn sabotage_subobjectpropertyof_chain_a_deleted_composed_edge_must_be_caught_with_index_and_witness()
 {
    let internal = common::load(CHAIN_FIXTURE);
    let (mut m, _) = owl_dl_verify::build_model(&internal, &Bounds::default()).expect("builds");
    let idx = chain_axiom_index(&internal);
    assert!(matches!(
        check_axiom(&internal.concepts, &m, idx, &internal.axioms[idx]),
        AxiomVerdict::Holds
    ));

    let t = internal.vocabulary.role_id("http://ex.org/t").expect("t");
    let u = internal.vocabulary.role_id("http://ex.org/u").expect("u");
    let v = internal.vocabulary.role_id("http://ex.org/v").expect("v");
    let c = internal.vocabulary.class_id("http://ex.org/C").expect("C");
    let elem_c = m.element_of_class(c).expect("C is satisfiable");
    // Both expansion paths run (`expand` fact-driven, `expand_from_axioms`
    // axiom-driven — see `build_model`'s doc), and they label the SAME
    // conceptual `∃t.∃u.A` witness differently (the fact path's Tseitin
    // marker carries its own reflexive label; the axiom path's opaque
    // intermediate carries an empty one), so `C` genuinely has more than one
    // t-successor. Find the specific (w, z) pair that is actually the
    // composed chain's witness, rather than assuming uniqueness.
    let (w, z) = m
        .successors(elem_c, t)
        .into_iter()
        .find_map(|w| {
            m.successors(w, u)
                .into_iter()
                .find(|&z| m.has_edge(elem_c, v, z))
                .map(|z| (w, z))
        })
        .expect("must find a t-then-u witness chain whose composed v-edge exists");

    m.test_only_remove_edge(v, elem_c, z);

    match check_axiom(&internal.concepts, &m, idx, &internal.axioms[idx]) {
        AxiomVerdict::Fails { witness, .. } => {
            assert_eq!(
                witness,
                vec![elem_c, w, z],
                "witness must be the composed chain's 3 elements"
            );
        }
        other => panic!("mutation must be caught, got {other:?}"),
    }
}

const EQUIVPROP_FIXTURE: &str = r"Prefix(:=<http://ex.org/>)
Ontology(<http://ex.org/equivprop>
Declaration(Class(:C)) Declaration(Class(:D))
Declaration(ObjectProperty(:p)) Declaration(ObjectProperty(:q)) Declaration(ObjectProperty(:z))
EquivalentObjectProperties(:p :q)
SubClassOf(:C ObjectSomeValuesFrom(:p :D))
)
";

#[test]
fn equivalent_object_properties_holds_on_a_healthy_ontology() {
    let internal = common::load(EQUIVPROP_FIXTURE);
    let (m, _) = owl_dl_verify::build_model(&internal, &Bounds::default()).expect("builds");
    let p = internal.vocabulary.role_id("http://ex.org/p").expect("p");
    assert!(
        !m.edges(p).is_empty(),
        "fixture must actually produce a p-edge, or Holds is vacuous truth"
    );
    let idx = axiom_index(&internal, |ax| {
        matches!(ax, Axiom::EquivalentObjectProperties(_))
    });
    assert!(matches!(
        check_axiom(&internal.concepts, &m, idx, &internal.axioms[idx]),
        AxiomVerdict::Holds
    ));
}

/// Same structural argument (and the same escape hatch) as
/// `SubObjectPropertyOf(Role)`: `build_role_hierarchy` registers `p ≡ q` in
/// BOTH directions from this exact axiom, so `has_edge(q)` already unions in
/// `p`'s bucket. Sabotage via a mismatched axiom (`p`, `z` — unrelated in
/// this ontology) instead of edge deletion.
#[test]
fn sabotage_equivalent_object_properties_a_mismatched_member_must_be_caught_with_index_and_witness()
{
    let internal = common::load(EQUIVPROP_FIXTURE);
    let (m, _) = owl_dl_verify::build_model(&internal, &Bounds::default()).expect("builds");
    let idx = axiom_index(&internal, |ax| {
        matches!(ax, Axiom::EquivalentObjectProperties(_))
    });

    let p = internal.vocabulary.role_id("http://ex.org/p").expect("p");
    let z = internal.vocabulary.role_id("http://ex.org/z").expect("z");
    let edges = m.edges(p);
    assert_eq!(edges.len(), 1, "fixture must have exactly one p-edge");
    let (from, to) = edges[0];

    let mismatched = Axiom::EquivalentObjectProperties(vec![Role::named(p), Role::named(z)]);
    match check_axiom(&internal.concepts, &m, idx, &mismatched) {
        AxiomVerdict::Fails { witness, .. } => {
            assert_eq!(
                witness,
                vec![from, to],
                "witness must be the mismatched edge's endpoints"
            );
        }
        other => panic!("mismatched axiom must be caught, got {other:?}"),
    }
}

#[test]
fn equivalent_object_properties_edge_deletion_is_structurally_a_no_op() {
    let internal = common::load(EQUIVPROP_FIXTURE);
    let (mut m, _) = owl_dl_verify::build_model(&internal, &Bounds::default()).expect("builds");
    let idx = axiom_index(&internal, |ax| {
        matches!(ax, Axiom::EquivalentObjectProperties(_))
    });
    let p = internal.vocabulary.role_id("http://ex.org/p").expect("p");
    let edges = m.edges(p);
    assert_eq!(edges.len(), 1);
    let (from, to) = edges[0];

    m.test_only_remove_edge(p, from, to);
    assert!(m.edges(p).is_empty());
    assert!(
        matches!(
            check_axiom(&internal.concepts, &m, idx, &internal.axioms[idx]),
            AxiomVerdict::Holds
        ),
        "deleting the only p-edge cannot produce a violation: the antecedent is now empty too"
    );
}

const TRANSITIVE_FIXTURE: &str = r"Prefix(:=<http://ex.org/>)
Ontology(<http://ex.org/trans9>
Declaration(Class(:C)) Declaration(Class(:A))
Declaration(ObjectProperty(:r))
TransitiveObjectProperty(:r)
SubClassOf(:C ObjectSomeValuesFrom(:r ObjectSomeValuesFrom(:r :A)))
)
";

#[test]
fn transitiverole_holds_on_a_healthy_ontology() {
    let internal = common::load(TRANSITIVE_FIXTURE);
    let (m, _) = owl_dl_verify::build_model(&internal, &Bounds::default()).expect("builds");
    let r = internal.vocabulary.role_id("http://ex.org/r").expect("r");
    assert!(
        !m.edges(r).is_empty(),
        "fixture must actually produce r-edges, or Holds is vacuous truth"
    );
    let idx = axiom_index(&internal, |ax| matches!(ax, Axiom::TransitiveRole(_)));
    assert!(matches!(
        check_axiom(&internal.concepts, &m, idx, &internal.axioms[idx]),
        AxiomVerdict::Holds
    ));
}

#[test]
#[allow(clippy::many_single_char_names)] // r, w, z mirror the fixture's own role/element names
fn sabotage_transitiverole_a_deleted_composed_edge_must_be_caught_with_index_and_witness() {
    let internal = common::load(TRANSITIVE_FIXTURE);
    let (mut m, _) = owl_dl_verify::build_model(&internal, &Bounds::default()).expect("builds");
    let idx = axiom_index(&internal, |ax| matches!(ax, Axiom::TransitiveRole(_)));
    assert!(matches!(
        check_axiom(&internal.concepts, &m, idx, &internal.axioms[idx]),
        AxiomVerdict::Holds
    ));

    let r = internal.vocabulary.role_id("http://ex.org/r").expect("r");
    let c = internal.vocabulary.class_id("http://ex.org/C").expect("C");
    let elem_c = m.element_of_class(c).expect("C is satisfiable");
    // As in the Chain sabotage above: `C` has more than one r-successor
    // (both expansion paths run and label the same nested-existential
    // witness differently), so find the specific (w, z) pair that is
    // actually the composed transitive witness rather than assuming
    // uniqueness.
    let (w, z) = m
        .successors(elem_c, r)
        .into_iter()
        .find_map(|w| {
            m.successors(w, r)
                .into_iter()
                .find(|&z| m.has_edge(elem_c, r, z))
                .map(|z| (w, z))
        })
        .expect("must find an r-then-r witness chain whose composed edge exists");

    m.test_only_remove_edge(r, elem_c, z);

    match check_axiom(&internal.concepts, &m, idx, &internal.axioms[idx]) {
        AxiomVerdict::Fails { witness, .. } => {
            assert_eq!(
                witness,
                vec![elem_c, w, z],
                "witness must be the composed transitive triple"
            );
        }
        other => panic!("mutation must be caught, got {other:?}"),
    }
}

// --- check_axiom: the two GUARDED variants (SymmetricRole, InverseObjectProperties) ---
//
// The reasoner's fragment gate admits these axioms only when a whole-ontology
// observability analysis proves the role unread, i.e. it should carry no
// edges at all. So the check here VERIFIES emptiness rather than checking
// symmetry/inverse semantics directly: a non-empty extension indicts that
// gate's analysis, not this closure — hence `Unresolved`, never `Fails`.

const SYMMETRIC_BARE_FIXTURE: &str = r"Prefix(:=<http://ex.org/>)
Ontology(<http://ex.org/symbare>
Declaration(ObjectProperty(:r))
SymmetricObjectProperty(:r)
)
";

const SYMMETRIC_WITH_EDGES_FIXTURE: &str = r"Prefix(:=<http://ex.org/>)
Ontology(<http://ex.org/symedge>
Declaration(Class(:C)) Declaration(Class(:D))
Declaration(ObjectProperty(:r))
SymmetricObjectProperty(:r)
SubClassOf(:C ObjectSomeValuesFrom(:r :D))
)
";

#[test]
fn bare_symmetric_role_with_an_empty_extension_holds() {
    let internal = common::load(SYMMETRIC_BARE_FIXTURE);
    let (m, _) = owl_dl_verify::build_model(&internal, &Bounds::default()).expect("builds");
    let idx = axiom_index(&internal, |ax| matches!(ax, Axiom::SymmetricRole(_)));
    assert!(matches!(
        check_axiom(&internal.concepts, &m, idx, &internal.axioms[idx]),
        AxiomVerdict::Holds
    ));
}

#[test]
fn a_guarded_role_that_has_edges_is_reported_not_accepted() {
    // The gate admits SymmetricRole only when a BareRoleDecls-style analysis
    // proves the role unread, so it should have no edges. Verify emptiness
    // rather than assume it: a non-empty extension means the observability
    // analysis is wrong, which is itself a finding.
    let internal = common::load(SYMMETRIC_WITH_EDGES_FIXTURE);
    let (m, _) = owl_dl_verify::build_model(&internal, &Bounds::default()).expect("builds");
    let r = internal.vocabulary.role_id("http://ex.org/r").expect("r");
    assert!(
        !m.edges(r).is_empty(),
        "fixture must actually give r edges, or this test proves nothing"
    );
    let idx = axiom_index(&internal, |ax| matches!(ax, Axiom::SymmetricRole(_)));
    match check_axiom(&internal.concepts, &m, idx, &internal.axioms[idx]) {
        AxiomVerdict::Unresolved(UnresolvedReason::GuardedRoleHasEdges { role }) => {
            assert_eq!(role, r);
        }
        other => {
            panic!("a non-empty extension must be Unresolved(GuardedRoleHasEdges), got {other:?}")
        }
    }
}

const INVERSE_BARE_FIXTURE: &str = r"Prefix(:=<http://ex.org/>)
Ontology(<http://ex.org/invbare>
Declaration(ObjectProperty(:p)) Declaration(ObjectProperty(:q))
InverseObjectProperties(:p :q)
)
";

const INVERSE_Q_HAS_EDGES_FIXTURE: &str = r"Prefix(:=<http://ex.org/>)
Ontology(<http://ex.org/invq>
Declaration(Class(:C)) Declaration(Class(:D))
Declaration(ObjectProperty(:p)) Declaration(ObjectProperty(:q))
InverseObjectProperties(:p :q)
SubClassOf(:C ObjectSomeValuesFrom(:q :D))
)
";

const INVERSE_P_HAS_EDGES_FIXTURE: &str = r"Prefix(:=<http://ex.org/>)
Ontology(<http://ex.org/invp>
Declaration(Class(:C)) Declaration(Class(:D))
Declaration(ObjectProperty(:p)) Declaration(ObjectProperty(:q))
InverseObjectProperties(:p :q)
SubClassOf(:C ObjectSomeValuesFrom(:p :D))
)
";

#[test]
fn inverse_object_properties_with_both_empty_holds() {
    let internal = common::load(INVERSE_BARE_FIXTURE);
    let (m, _) = owl_dl_verify::build_model(&internal, &Bounds::default()).expect("builds");
    let idx = axiom_index(&internal, |ax| {
        matches!(ax, Axiom::InverseObjectProperties(_, _))
    });
    assert!(matches!(
        check_axiom(&internal.concepts, &m, idx, &internal.axioms[idx]),
        AxiomVerdict::Holds
    ));
}

/// The gate requires BOTH `p` and `q` unread; a check that looked only at
/// `p` would accept this model, where `q` (not `p`) has edges.
#[test]
fn inverse_object_properties_checks_both_roles_q_has_edges_is_caught() {
    let internal = common::load(INVERSE_Q_HAS_EDGES_FIXTURE);
    let (m, _) = owl_dl_verify::build_model(&internal, &Bounds::default()).expect("builds");
    let q = internal.vocabulary.role_id("http://ex.org/q").expect("q");
    assert!(!m.edges(q).is_empty(), "fixture must actually give q edges");
    let idx = axiom_index(&internal, |ax| {
        matches!(ax, Axiom::InverseObjectProperties(_, _))
    });
    match check_axiom(&internal.concepts, &m, idx, &internal.axioms[idx]) {
        AxiomVerdict::Unresolved(UnresolvedReason::GuardedRoleHasEdges { role }) => {
            assert_eq!(role, q, "a check that only looked at p would miss this");
        }
        other => panic!("q having edges must be caught, got {other:?}"),
    }
}

/// The mirror control: `p` (not `q`) has edges, proving the check does not
/// pass simply because it happens to always report `q`.
#[test]
fn inverse_object_properties_checks_both_roles_p_has_edges_is_caught() {
    let internal = common::load(INVERSE_P_HAS_EDGES_FIXTURE);
    let (m, _) = owl_dl_verify::build_model(&internal, &Bounds::default()).expect("builds");
    let p = internal.vocabulary.role_id("http://ex.org/p").expect("p");
    assert!(!m.edges(p).is_empty(), "fixture must actually give p edges");
    let idx = axiom_index(&internal, |ax| {
        matches!(ax, Axiom::InverseObjectProperties(_, _))
    });
    match check_axiom(&internal.concepts, &m, idx, &internal.axioms[idx]) {
        AxiomVerdict::Unresolved(UnresolvedReason::GuardedRoleHasEdges { role }) => {
            assert_eq!(role, p);
        }
        other => panic!("p having edges must be caught, got {other:?}"),
    }
}

// --- check_axiom: the unhandled-variant LOOP ---

/// One sample per wholly-unhandled variant (12), plus `Chain` of length 1 and
/// 3 (a length-2 `Chain` is checked; anything else is not) and
/// `EquivalentObjectProperties` containing an inverse (also not checked).
/// `top`/role/individual ids are dummies: none of these arms touch the pool
/// or the model at all, they return `Unresolved` immediately.
fn unhandled_axiom_samples(top: ConceptId) -> Vec<Axiom> {
    let c0 = ClassId::new(0);
    let r0 = RoleId::new(0);
    let r1 = RoleId::new(1);
    let ind0 = IndividualId::new(0);
    let ind1 = IndividualId::new(1);
    vec![
        // The 12 wholly-unhandled variants.
        Axiom::DisjointUnion {
            class: c0,
            members: vec![top, top],
        },
        Axiom::DisjointObjectProperties(vec![Role::named(r0), Role::named(r1)]),
        Axiom::AsymmetricRole(Role::named(r0)),
        Axiom::ReflexiveRole(Role::named(r0)),
        Axiom::IrreflexiveRole(Role::named(r0)),
        Axiom::FunctionalRole(Role::named(r0)),
        Axiom::InverseFunctionalRole(Role::named(r0)),
        Axiom::ClassAssertion {
            class: top,
            individual: ind0,
        },
        Axiom::ObjectPropertyAssertion {
            role: Role::named(r0),
            subject: ind0,
            object: ind1,
        },
        Axiom::NegativeObjectPropertyAssertion {
            role: Role::named(r0),
            subject: ind0,
            object: ind1,
        },
        Axiom::SameIndividual(vec![ind0, ind1]),
        Axiom::DifferentIndividuals(vec![ind0, ind1]),
        // Chain of the wrong length.
        Axiom::SubObjectPropertyOf {
            sub: SubRolePath::Chain(vec![Role::named(r0)]),
            sup: Role::named(r1),
        },
        Axiom::SubObjectPropertyOf {
            sub: SubRolePath::Chain(vec![Role::named(r0), Role::named(r1), Role::named(r0)]),
            sup: Role::named(r1),
        },
        // EquivalentObjectProperties containing an inverse.
        Axiom::EquivalentObjectProperties(vec![Role::named(r0), Role::inverse(r1)]),
    ]
}

#[test]
fn every_unhandled_axiom_variant_yields_unresolved() {
    let mut pool = ConceptPool::new();
    let top = pool.top();
    let model = StubModel::new(1);
    let samples = unhandled_axiom_samples(top);
    assert_eq!(
        samples.len(),
        15,
        "12 wholly-unhandled variants + Chain(len 1) + Chain(len 3) + \
         EquivalentObjectProperties(Inverse)"
    );
    for ax in samples {
        assert!(
            matches!(
                check_axiom(&pool, &model, 0, &ax),
                AxiomVerdict::Unresolved(_)
            ),
            "unhandled variant {ax:?} must be Unresolved, never a silent pass"
        );
    }
}

// -----------------------------------------------------------------------
// Task 10: `verify`, `Verdict`, and the `VerifiedModel` type-state.
// -----------------------------------------------------------------------
//
// Everything above this point exercises `check_axiom`/`eval_concept`
// directly. These tests exercise the assembled surface: `verify` running
// `check_axiom` over a WHOLE ontology's axiom list and folding the results
// into one `Verdict`.

// A second role (`:q`) plus `DisjointObjectProperties`, so this fixture
// carries one CHECKED axiom (`SubClassOf`, which holds) and one variant
// `check_axiom` never judges at all (`DisjointObjectProperties` has no
// derived side-axiom at conversion, unlike e.g. `FunctionalObjectProperty` —
// confirmed by probing `convert_ontology`'s output directly — so this
// fixture's axiom list is exactly the six printed below and nothing this
// test did not ask for).
const UNHANDLED_VARIANT_FIXTURE: &str = r"Prefix(:=<http://ex.org/>)
Ontology(<http://ex.org/unresolved>
Declaration(Class(:A)) Declaration(Class(:B))
Declaration(ObjectProperty(:p)) Declaration(ObjectProperty(:q))
SubClassOf(:A :B)
DisjointObjectProperties(:p :q)
)
";

#[test]
fn verify_returns_verified_and_some_model_on_a_healthy_ontology() {
    let internal = common::load(SUBCLASS_FIXTURE);
    let (m, build_reasons) =
        owl_dl_verify::build_model(&internal, &Bounds::default()).expect("builds");
    assert!(
        build_reasons.is_empty(),
        "this fixture must build cleanly, or the verdict below would not \
         actually be exercising `verify`'s own logic: {build_reasons:?}"
    );
    let expected_domain = m.domain_size();
    let expected_axioms = internal.axioms.len();

    let (verdict, model_out) = verify(m, &internal, None);
    match verdict {
        Verdict::Verified {
            axioms_checked,
            domain_size,
        } => {
            assert_eq!(
                domain_size, expected_domain,
                "domain_size must be the model's actual domain size"
            );
            assert_eq!(
                axioms_checked, expected_axioms,
                "a Verified run checked every axiom, so axioms_checked must \
                 equal the ontology's whole axiom count"
            );
        }
        other => panic!("a healthy ontology must verify: {other:?}"),
    }
    assert!(
        model_out.is_some(),
        "Verified must hand back Some(VerifiedModel) — that is the only \
         verdict allowed to"
    );
}

#[test]
fn verify_returns_violated_and_none_on_a_sabotaged_label_and_pins_the_witness() {
    let internal = common::load(SUBCLASS_FIXTURE);
    let (mut m, _) = owl_dl_verify::build_model(&internal, &Bounds::default()).expect("builds");
    let idx = axiom_index(&internal, |ax| matches!(ax, Axiom::SubClassOf { .. }));
    let a = internal.vocabulary.class_id("http://ex.org/A").expect("A");
    let b = internal.vocabulary.class_id("http://ex.org/B").expect("B");
    let elem_a = m.element_of_class(a).expect("A is satisfiable");
    m.test_only_remove_from_label(elem_a, b);
    let expected_domain = m.domain_size();

    let (verdict, model_out) = verify(m, &internal, None);
    match verdict {
        Verdict::Violated {
            domain_size,
            violations,
            unresolved,
        } => {
            assert_eq!(domain_size, expected_domain, "domain_size on Violated");
            assert_eq!(violations.len(), 1, "exactly one axiom was sabotaged");
            assert_eq!(violations[0].axiom_index, idx, "pinned axiom index");
            assert_eq!(
                violations[0].axiom, internal.axioms[idx],
                "the reported axiom must be the SubClassOf that was sabotaged"
            );
            assert_eq!(
                violations[0].witness,
                vec![elem_a],
                "pinned witness element"
            );
            assert!(
                unresolved.is_empty(),
                "this fixture has nothing else to be unresolved about"
            );
        }
        other => panic!("the sabotaged label must be caught as Violated: {other:?}"),
    }
    assert!(
        model_out.is_none(),
        "Violated must never hand back a VerifiedModel"
    );
}

#[test]
fn verify_returns_unresolved_and_none_on_an_unhandled_axiom_variant() {
    let internal = common::load(UNHANDLED_VARIANT_FIXTURE);
    let (m, build_reasons) =
        owl_dl_verify::build_model(&internal, &Bounds::default()).expect("builds");
    assert!(build_reasons.is_empty(), "{build_reasons:?}");
    let expected_domain = m.domain_size();
    let idx = axiom_index(&internal, |ax| {
        matches!(ax, Axiom::DisjointObjectProperties(_))
    });

    let (verdict, model_out) = verify(m, &internal, None);
    match verdict {
        Verdict::Unresolved {
            domain_size,
            reasons,
        } => {
            assert_eq!(domain_size, expected_domain, "domain_size on Unresolved");
            assert!(
                reasons.iter().any(|r| matches!(
                    r,
                    UnresolvedReason::UnhandledAxiom {
                        axiom_index,
                        variant: "DisjointObjectProperties",
                    } if *axiom_index == idx
                )),
                "the unhandled DisjointObjectProperties axiom must be reported, \
                 pinned to its own index: {reasons:?}"
            );
        }
        other => panic!("an unhandled axiom variant must be Unresolved: {other:?}"),
    }
    assert!(
        model_out.is_none(),
        "Unresolved must never hand back a VerifiedModel — even though the \
         model itself was never sabotaged, some axiom went unjudged, and \
         that alone must be enough to withhold the type-state wrapper"
    );
}

#[test]
fn violated_outranks_unresolved_and_still_carries_the_unresolved_rows() {
    // Same fixture as the unhandled-variant test (so it carries a genuine
    // UnhandledAxiom), PLUS the SubClassOf label sabotage from the
    // Violated test — so one run produces both kinds of finding at once.
    let internal = common::load(UNHANDLED_VARIANT_FIXTURE);
    let (mut m, _) = owl_dl_verify::build_model(&internal, &Bounds::default()).expect("builds");
    let sc_idx = axiom_index(&internal, |ax| matches!(ax, Axiom::SubClassOf { .. }));
    let unhandled_idx = axiom_index(&internal, |ax| {
        matches!(ax, Axiom::DisjointObjectProperties(_))
    });
    let a = internal.vocabulary.class_id("http://ex.org/A").expect("A");
    let b = internal.vocabulary.class_id("http://ex.org/B").expect("B");
    let elem_a = m.element_of_class(a).expect("A is satisfiable");
    m.test_only_remove_from_label(elem_a, b);

    let (verdict, model_out) = verify(m, &internal, None);
    match verdict {
        Verdict::Violated {
            violations,
            unresolved,
            ..
        } => {
            assert_eq!(violations.len(), 1);
            assert_eq!(violations[0].axiom_index, sc_idx);
            assert!(
                !unresolved.is_empty(),
                "coverage must not be hidden behind the violation — the \
                 unhandled DisjointObjectProperties finding must still be \
                 reported: {unresolved:?}"
            );
            assert!(unresolved.iter().any(|r| matches!(
                r,
                UnresolvedReason::UnhandledAxiom {
                    axiom_index,
                    variant: "DisjointObjectProperties",
                } if *axiom_index == unhandled_idx
            )));
        }
        other => panic!("Violated must outrank Unresolved: {other:?}"),
    }
    assert!(model_out.is_none());
}

#[test]
fn violation_witness_containing_a_tseitin_shaped_class_id_renders_without_panicking() {
    // `check_axiom`/`eval_concept` never touch `Vocabulary` at all (see
    // `eval.rs`'s module doc), so this element is manufactured directly on
    // the model rather than through the saturator — the point being tested
    // is `verify`'s OWN rendering, not how such a label could arise in
    // practice. `FiniteModel::intern` is a plain public method (not gated
    // behind the test-mutations feature): interning a label a caller built
    // by hand is ordinary usage, unlike mutating one already interned.
    let internal = common::load(SUBCLASS_FIXTURE);
    let (mut m, _) = owl_dl_verify::build_model(&internal, &Bounds::default()).expect("builds");
    let a = internal.vocabulary.class_id("http://ex.org/A").expect("A");
    // Well beyond anything `internal`'s vocabulary itself interned — stands
    // in for a Tseitin marker (`TseitinAllocator` bases marker ids at
    // `vocabulary.num_classes()`) or an `inject_conjunction`-created
    // `verify-aug:` class. `Vocabulary::class_iri` indexes its own table
    // directly and PANICS on an id outside it; this is exactly the hazard
    // `verify`'s witness rendering exists to avoid.
    let synthetic =
        ClassId::new(u32::try_from(internal.vocabulary.num_classes()).unwrap_or(u32::MAX) + 1000);
    let tainted = m.intern(vec![a, synthetic]);
    let idx = axiom_index(&internal, |ax| matches!(ax, Axiom::SubClassOf { .. }));

    // Does not panic: that is the assertion. `verify` must render `tainted`'s
    // label — which contains `synthetic` — while `m` is still alive, without
    // ever calling `class_iri(synthetic)`.
    let (verdict, model_out) = verify(m, &internal, None);
    match verdict {
        Verdict::Violated { violations, .. } => {
            let hit = violations
                .iter()
                .find(|v| v.axiom_index == idx && v.witness.contains(&tainted))
                .expect("the tainted element must be reported as the witness");
            assert!(
                hit.note.contains("http://ex.org/A"),
                "the real class in the label must still render by IRI: {}",
                hit.note
            );
            assert!(
                hit.note.contains("synthetic"),
                "the Tseitin-shaped id must render by a synthetic tag: {}",
                hit.note
            );
        }
        other => panic!("the tainted element must produce a violation: {other:?}"),
    }
    assert!(model_out.is_none());
}

// --- Type-state structural checks (no `trybuild` dependency; see this
// task's brief) -------------------------------------------------------------
//
// The "FiniteModel has no still_holds_after" property now has a REAL
// compile check — `src/model.rs`'s `compile_fail` doctest on `FiniteModel`
// — rather than a source scan; run it with the doctests
// (`cargo test -p owl-dl-verify`, "Doc-tests owl_dl_verify" section). The
// two checks below cover what a doctest can't: field privacy (a private
// field is enforced by the compiler on every build, but pinning that it
// STAYS private is still worth a named test) and a temporal marker for
// Task 11, split into its own test rather than folded into the field-
// privacy one so its lifecycle is unambiguous.

#[test]
fn verified_model_does_not_expose_its_inner_finite_model_mutably() {
    // The type-state guarantee rests entirely on `VerifiedModel`'s tuple
    // field being PRIVATE: a `pub struct VerifiedModel(pub FiniteModel)`
    // would let any caller reconstruct one by hand — bypassing `verify`'s
    // checking loop entirely — or reach the wrapped model mutably through
    // it. This is a permanent invariant, NOT something Task 11 should touch:
    // `still_holds_after` must be added as a method that takes `&self`, and
    // doing so does not require (and must not motivate) making the field
    // `pub` or adding an `into_inner`/`&mut` accessor.
    let lib_src = include_str!("../src/lib.rs");
    assert!(
        lib_src.contains("struct VerifiedModel("),
        "VerifiedModel must exist as a tuple struct"
    );
    assert!(
        !lib_src.contains("VerifiedModel(pub "),
        "VerifiedModel's tuple field must stay private, or any caller could \
         construct one without going through verify()'s checking loop"
    );
}

#[test]
fn verify_honours_an_already_elapsed_deadline_and_reports_it_as_a_deadline_not_a_count() {
    // Requirement: `limit: None` is what distinguishes a deadline-based
    // `BoundTripped` from a count-based one (e.g. `max_rounds`,
    // `max_elements`). A refactor could swap the bound name or the `None`
    // for `Some(0)` without any other test noticing, since every other
    // `verify` test in this file passes `None` for `deadline`.
    let internal = common::load(SUBCLASS_FIXTURE);
    let (m, build_reasons) =
        owl_dl_verify::build_model(&internal, &Bounds::default()).expect("builds");
    assert!(build_reasons.is_empty(), "{build_reasons:?}");
    let expected_domain = m.domain_size();

    // Already in the past: `verify` must trip on the very first axiom.
    let elapsed = std::time::Instant::now()
        .checked_sub(std::time::Duration::from_secs(1))
        .unwrap_or_else(std::time::Instant::now);
    let (verdict, model_out) = verify(m, &internal, Some(elapsed));
    match verdict {
        Verdict::Unresolved {
            domain_size,
            reasons,
        } => {
            assert_eq!(domain_size, expected_domain);
            assert!(
                reasons.contains(&UnresolvedReason::BoundTripped {
                    bound: "deadline",
                    limit: None,
                }),
                "an elapsed deadline must report BoundTripped{{bound: \
                 \"deadline\", limit: None}}, not a count-shaped variant: \
                 {reasons:?}"
            );
        }
        other => panic!("an already-elapsed deadline must yield Unresolved: {other:?}"),
    }
    assert!(
        model_out.is_none(),
        "a deadline-truncated run must never hand back a VerifiedModel"
    );
}
