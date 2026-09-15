//! Does a SUB-ROLE edge wake a clause whose body wants the SUPER-role?
//!
//! `index_one_clause` indexes a Horn clause under its body atom's OWN role id
//! (`push_role_trigger(role_id_index(*r), ci)` — the hierarchy is consulted only for
//! `is_symmetric`), and the `Event::Edge` dispatch looks up `role_id_index(role)`,
//! the edge's own role. So a clause with body `S(x,y)` lives under key `S`, while an
//! edge `R(a,b)` dispatches on key `R`. With the hierarchy supplied (`with_sub_roles`,
//! i.e. `RUSTDL_CLASSIFY_ROLE_HIERARCHY`), `role_matches` WOULD accept that edge for
//! the `S`-atom — but only if something wakes the clause in the first place.
//!
//! This matters only when the hierarchy is on: with it off, an `R`-edge genuinely does
//! not satisfy an `S`-atom, so there is nothing to miss.
//!
//! # Why this is a tableau-level test
//!
//! A first attempt at the same question through the CLI was answered by the EL
//! saturator before the wedge ever saw it, so it proved nothing about the wedge. These
//! tests drive `HyperEngine` directly.
//!
//! # The shape
//!
//! `S(x,y) → Q(x)` has NO class atom on `y`, so `succ_trigger` cannot cover it and
//! `role_trigger[S]` is its only wake-up. That is the whole point: a body with a class
//! atom on the successor would be woken by the successor's `Event::Label` regardless,
//! which would mask the gap.

#![allow(clippy::unwrap_used)]

use owl_dl_core::clause::{Atom, DlClause, X};
use owl_dl_core::ir::{ClassId, Role, RoleId};
use owl_dl_core::role_hierarchy::RoleHierarchyBuilder;
use owl_dl_tableau::hyper::{HyperEngine, HyperResult};

const A: u32 = 0;
const Q: u32 = 1;
const TOP: u32 = 2;
const D: u32 = 3;
const R: u32 = 0; // sub-role
const S: u32 = 1; // super-role

fn cls(i: u32) -> ClassId {
    ClassId::new(i)
}

/// `A ⊑ ∃R.⊤`, `S(x,y) → Q(x)`, `A ⊓ Q → ⊥`, with `R ⊑ S`.
///
/// With the hierarchy, the `R`-edge satisfies the `S`-atom, so `Q` is derived on the
/// root and clashes with `A` ⇒ **Unsat**. A `Sat` verdict means the `S`-clause was
/// never fired — the trigger gap.
fn clauses() -> Vec<DlClause> {
    vec![
        // A → ∃R.⊤
        DlClause {
            body: vec![Atom::Class(cls(A), X)],
            head: vec![Atom::Exists(Role::named(RoleId::new(R)), cls(TOP), X)],
        },
        // S(x,y) → Q(x)   — no class atom on y, so role_trigger[S] is the only wake-up
        DlClause {
            body: vec![Atom::Role(Role::named(RoleId::new(S)), X, 1)],
            head: vec![Atom::Class(cls(Q), X)],
        },
        // A ⊓ Q → ⊥
        DlClause {
            body: vec![Atom::Class(cls(A), X), Atom::Class(cls(Q), X)],
            head: vec![],
        },
    ]
}

fn hierarchy() -> owl_dl_core::role_hierarchy::RoleHierarchy {
    let mut b = RoleHierarchyBuilder::with_roles(2);
    b.add_sub_role(RoleId::new(R), RoleId::new(S));
    b.build()
}

#[test]
fn control_without_the_hierarchy_it_is_satisfiable() {
    // CONTROL. No hierarchy ⇒ an `R`-edge does not satisfy an `S`-atom, so `Q` is
    // genuinely not derivable and `Sat` is CORRECT. Without this control, the test
    // below could pass for the wrong reason (e.g. a fixture that is Unsat anyway).
    let cl = clauses();
    let mut eng = HyperEngine::new(&cl, cls(A));
    assert_eq!(
        eng.decide(64),
        HyperResult::Sat,
        "with no role hierarchy, R does not satisfy S — Sat is the correct answer"
    );
}

#[test]
fn a_sub_role_edge_wakes_a_super_role_body_clause() {
    // THE FIX (#128). `R ⊑ S`, so the edge `R(root, y)` satisfies the `S`-atom, `Q`
    // lands on the root and clashes with `A` ⇒ `Unsat`.
    //
    // This was a PINNED GAP until the sub-role widening landed: `index_one_clause`
    // filed the clause under `role_trigger[S]` (its body atom's own role) while the
    // `Event::Edge` for an `R`-edge dispatched on `role_trigger[R]`, so nothing ever
    // woke it and the engine answered `Sat`. `role_matches` would have accepted the
    // edge — it was simply never asked. The widening files a role body under every
    // sub-role as well, so the dispatch finds it.
    //
    // The gap was never reachable through the pipeline (the EL saturator answers
    // `ObjectPropertyDomain`, which is `∃S.⊤ ⊑ C`), which is why this has to be a
    // tableau-level test: an end-to-end fixture is answered before the wedge sees it
    // and proves nothing either way.
    let cl = clauses();
    let mut eng = HyperEngine::new(&cl, cls(A)).with_sub_roles(hierarchy());
    assert_eq!(
        eng.decide(64),
        HyperResult::Unsat,
        "R ⊑ S, so the R-edge satisfies the S-atom: Q must be derived and clash with A"
    );
}

/// The INVERSE first-leg sibling of the same defect.
///
/// `index_one_clause` files an inverse/symmetric first-leg atom into
/// `inverse_first_trigger` under `role_id_index(*r)` — its OWN role — and the
/// `Event::Edge` dispatch looks that table up under the EDGE's role id, exactly as it
/// does for `role_trigger`. So the sub-role widening has to cover this table too, or
/// the same miss survives one indirection away.
///
/// Shape: `S⁻(x,y) → Q(x)` — "x has an `S`-predecessor" — which is what
/// `ObjectPropertyRange(S, Q)` becomes at the far endpoint. An `R`-edge with `R ⊑ S`
/// must wake it AT THE TARGET.
fn inverse_leg_clauses() -> Vec<DlClause> {
    vec![
        // A → ∃R.D
        DlClause {
            body: vec![Atom::Class(cls(A), X)],
            head: vec![Atom::Exists(Role::named(RoleId::new(R)), cls(D), X)],
        },
        // S⁻(x,y) → Q(x)   — fires at the edge TARGET via inverse_first_trigger
        DlClause {
            body: vec![Atom::Role(Role::inverse(RoleId::new(S)), X, 1)],
            head: vec![Atom::Class(cls(Q), X)],
        },
        // Q ⊓ D → ⊥
        DlClause {
            body: vec![Atom::Class(cls(Q), X), Atom::Class(cls(D), X)],
            head: vec![],
        },
    ]
}

#[test]
fn control_inverse_leg_without_the_hierarchy_is_satisfiable() {
    // CONTROL: no hierarchy ⇒ an `R`-edge does not supply an `S⁻` leg, so `Sat` is
    // correct and the assertion below cannot pass for the wrong reason.
    let cl = inverse_leg_clauses();
    let mut eng = HyperEngine::new(&cl, cls(A));
    assert_eq!(eng.decide(64), HyperResult::Sat);
}

#[test]
fn a_sub_role_edge_wakes_an_inverse_first_leg_clause() {
    // `R ⊑ S`, so the edge `root —R→ y` gives `y` an `S⁻`-successor: `Q` lands on `y`,
    // which already carries `D`, and `Q ⊓ D → ⊥` clashes ⇒ `Unsat`.
    let cl = inverse_leg_clauses();
    let mut eng = HyperEngine::new(&cl, cls(A)).with_sub_roles(hierarchy());
    assert_eq!(
        eng.decide(64),
        HyperResult::Unsat,
        "a sub-role edge must also wake an INVERSE first-leg clause — \
         inverse_first_trigger is keyed by role id exactly like role_trigger, so it \
         needs the same sub-role widening"
    );
}
