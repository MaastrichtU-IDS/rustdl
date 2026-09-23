//! #135: does an INVERSE-polarity edge wake a POSITIVE first-leg clause at its
//! TARGET?
//!
//! `src —r⁻→ tgt` asserts `r(tgt, src)`, so a clause with body `r(X, v)` is
//! satisfiable at `tgt` — the matcher's predecessor scan handles the reversed
//! read (`role_matches(er.flip(), …)`). But `Event::Edge` fired `role_trigger`
//! only at the edge SOURCE, and `inverse_first_trigger` (fire-at-target) was
//! filed only for INVERSE or SYMMETRIC first legs. A positive first leg — a
//! chain `r ∘ p ⊑ p` is the reported case (#135) — was never woken at the
//! witness node a `∃r⁻` lowering created, so the wedge answered `Sat` for a
//! subsumed pair and `classify` silently omitted what `subclass` proves.
//!
//! Fixed by `pos_first_target_trigger`, the mirror table, whose dispatch is
//! gated on the EDGE being inverse-polarity (a forward edge gives its target
//! only an inverse successor, which `inverse_first_trigger` already covers).
//!
//! Tableau-level for the same reason as `subrole_edge_trigger.rs`: end-to-end,
//! the canonicalised named-inverse fixtures are the reasoner tests' job
//! (`chain_symmetric_leg.rs` and the #135 canaries there).

#![allow(clippy::unwrap_used)]

use owl_dl_core::clause::{Atom, DlClause, X};
use owl_dl_core::ir::{ClassId, Role, RoleId};
use owl_dl_core::role_hierarchy::RoleHierarchyBuilder;
use owl_dl_tableau::hyper::{HyperEngine, HyperResult};

const A: u32 = 0;
const Q: u32 = 1;
const D: u32 = 2;
const R: u32 = 0;
const S: u32 = 1; // super-role of R in the widening test; unrelated in the key test
const T: u32 = 2; // unrelated role

fn cls(i: u32) -> ClassId {
    ClassId::new(i)
}

/// `A ⊑ ∃R⁻.D`, `R(x,y) → Q(x)`, `Q ⊓ D → ⊥`.
///
/// The witness `w` carries `D` and — via the inverse edge — `R(w, root)`, so the
/// positive-first-leg clause must fire AT `w`, derive `Q(w)`, and clash: **Unsat**.
/// `Sat` means the clause was never woken at the target — the #135 gap. The body
/// deliberately has no class atom on the successor, so no `Event::Label` can mask
/// a missing edge trigger (same discipline as `subrole_edge_trigger.rs`).
fn clauses(first_leg_role: u32) -> Vec<DlClause> {
    vec![
        DlClause {
            body: vec![Atom::Class(cls(A), X)],
            head: vec![Atom::Exists(Role::inverse(RoleId::new(R)), cls(D), X)],
        },
        DlClause {
            body: vec![Atom::Role(Role::named(RoleId::new(first_leg_role)), X, 1)],
            head: vec![Atom::Class(cls(Q), X)],
        },
        DlClause {
            body: vec![Atom::Class(cls(Q), X), Atom::Class(cls(D), X)],
            head: vec![],
        },
    ]
}

#[test]
fn an_inverse_edge_wakes_a_positive_first_leg_clause_at_the_target() {
    // THE #135 FIX, hierarchy-free: the gap does not need a role hierarchy at
    // all (the reported reproducer only needs the inverse-polarity edge the
    // canonicaliser produces from `InverseObjectProperties`).
    let cl = clauses(R);
    let mut eng = HyperEngine::new(&cl, cls(A));
    assert_eq!(
        eng.decide(64),
        HyperResult::Unsat,
        "R⁻(root, w) is R(w, root): the R-body clause must fire at w and clash Q ⊓ D"
    );
}

#[test]
fn an_unrelated_role_body_is_not_woken_into_a_false_clash() {
    // FP GUARD / key-confusion control: the first leg wants `T`, the edge is
    // `R⁻`, no hierarchy relates them — `Q` must not be derived anywhere.
    let cl = clauses(T);
    let mut eng = HyperEngine::new(&cl, cls(A));
    assert_eq!(
        eng.decide(64),
        HyperResult::Sat,
        "an R⁻-edge says nothing about T — deriving a clash here is a false positive"
    );
}

#[test]
fn the_sub_role_widening_reaches_the_new_table_too() {
    // `R ⊑ S`, first leg wants `S`: the `R⁻`-edge target has `R(w, root)` hence
    // `S(w, root)`. Without the hierarchy the same fixture is the control.
    let cl = clauses(S);
    let mut eng = HyperEngine::new(&cl, cls(A));
    assert_eq!(
        eng.decide(64),
        HyperResult::Sat,
        "control: with no hierarchy, R does not satisfy S — Sat is correct"
    );
    let mut b = RoleHierarchyBuilder::with_roles(3);
    b.add_sub_role(RoleId::new(R), RoleId::new(S));
    let mut eng = HyperEngine::new(&cl, cls(A)).with_sub_roles(b.build());
    assert_eq!(
        eng.decide(64),
        HyperResult::Unsat,
        "R ⊑ S: the widened filing must wake the S-body clause at the R⁻-edge target"
    );
}
