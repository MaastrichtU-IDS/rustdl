//! Tests for `eval::eval_concept`, driven by a hand-built `Interpretation`
//! stub rather than `FiniteModel`. That keeps these tests asserting
//! `eval_concept`'s own behaviour without depending on the builder's
//! correctness — the two are meant to be checked independently of each
//! other.

use std::collections::HashSet;

use owl_dl_core::{Axiom, ClassId, ConceptPool, InternalOntology, RoleId};
use owl_dl_verify::eval::{AxiomVerdict, Judgement, check_axiom, eval_concept};
use owl_dl_verify::{Bounds, Element, Interpretation};

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

/// First axiom in `internal.axioms` matching `pred`, or panics — a fixture
/// missing the axiom it was written to exercise is a test-authoring bug, not
/// something to silently tolerate.
fn axiom_index(internal: &InternalOntology, pred: impl Fn(&Axiom) -> bool) -> usize {
    internal
        .axioms
        .iter()
        .position(pred)
        .expect("fixture must contain the axiom this test exercises")
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
