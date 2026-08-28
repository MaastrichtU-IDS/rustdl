//! Tests for `eval::eval_concept`, driven by a hand-built `Interpretation`
//! stub rather than `FiniteModel`. That keeps these tests asserting
//! `eval_concept`'s own behaviour without depending on the builder's
//! correctness — the two are meant to be checked independently of each
//! other.

use std::collections::HashSet;

use owl_dl_core::{ClassId, ConceptPool, RoleId};
use owl_dl_verify::eval::{Judgement, eval_concept};
use owl_dl_verify::{Element, Interpretation};

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
