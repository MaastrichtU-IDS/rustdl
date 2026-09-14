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
fn pinned_gap_a_sub_role_edge_does_not_wake_a_super_role_body_clause() {
    // PINNED KNOWN GAP, deliberately not `#[ignore]`d — see
    // `docs/2026-08-18-ignored-sentinels-went-stale-unobserved.md`.
    //
    // The CORRECT answer here is `Unsat`: `R ⊑ S`, so the edge `R(root, y)` satisfies
    // the `S`-atom, `Q` lands on the root and clashes with `A`. `role_matches` agrees —
    // it would accept the edge. But nothing ever ASKS it: `index_one_clause` files this
    // clause under `role_trigger[S]` (its body atom's own role), while the `Event::Edge`
    // for an `R`-edge dispatches on `role_trigger[R]`. The clause is never woken, so the
    // engine reports `Sat`.
    //
    // # Why this is pinned rather than fixed
    //
    // It is **not reachable through the pipeline** as of 2026-09-14. Three attempts to
    // provoke it end-to-end all returned the correct answer, agreeing with Konclude:
    // an asserted ABox edge (`ObjectPropertyAssertion` + `ObjectPropertyDomain`), a
    // generated TBox edge (`A ⊑ ∃R.⊤`), and a disjunction-forced variant intended to
    // defeat the saturator. The classify banner explains why: `subsumption:
    // saturation=2 tableau=0` — the EL saturator answers these, because
    // `ObjectPropertyDomain(S, C)` is EL-expressible (`∃S.⊤ ⊑ C`) and so never needs
    // the wedge.
    //
    // That protection is an ACCIDENT of the saturator's coverage, not a property of the
    // wedge — the same shape as #145, where a horned-owl parser gap was the only thing
    // making an unsound reading unreachable. A clause with a role-atom body that the
    // saturator defers would expose this live.
    //
    // THE FIX, if it is ever needed: `index_one_clause` already receives the hierarchy
    // (`sym`), so a role-atom body on `r` can additionally be filed under every
    // `h.sub_roles(r.role_id())` at matching polarity. That widens the index (more
    // clauses woken per edge), which is exactly the Layer A cost profile, so it should
    // be measured, not assumed free.
    //
    // If this test starts FAILING, the gap has been closed — flip it to `Unsat` and
    // delete this note.
    let cl = clauses();
    let mut eng = HyperEngine::new(&cl, cls(A)).with_sub_roles(hierarchy());
    assert_eq!(
        eng.decide(64),
        HyperResult::Sat,
        "known gap: the correct answer is Unsat. A failure here means a sub-role edge \
         now wakes a super-role-body clause — the gap is closed, make this a positive"
    );
}
