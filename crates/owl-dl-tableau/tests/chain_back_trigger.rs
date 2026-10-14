//! Is a multi-role clause body woken when its LAST edge arrives?
//!
//! A clause like `r(x,y) ∧ s(y,z) → u(x,z)` is rooted at `x`. When the `s`-edge
//! arrives, `Event::Edge` fires `role_back_trigger` clauses at the PREDECESSORS of the
//! edge's source (`src.preds`). That finds the root only when the root reaches the
//! middle node through a forward edge stored at the root. These tests probe the cases
//! where it does not.
//!
//! Every body here has NO class atom on a non-root variable, so `succ_trigger` cannot
//! wake the clause and the edge dispatch is its only wake-up.
//!
//! The gap tests pin BOTH behaviours: with the #137 re-seed net off the drain
//! answers a wrong `Sat` (the gap, pinned so a fix FLIPS it), and at default settings the #137 re-seed net catches
//! both gaps before `Sat`, so they are not live answer bugs. The `pinned_*` tests
//! switch the net off per engine and check that `RUSTDL_HYPER_EDGE_PIN` keeps every
//! binding the existing triggers reach: pinning narrows which bindings an edge event
//! enumerates, never where a clause is fired.

#![allow(clippy::unwrap_used)]

use owl_dl_core::clause::{Atom, DlClause, X};
use owl_dl_core::ir::{ClassId, Role, RoleId};
use owl_dl_tableau::hyper::{HyperEngine, HyperResult};

const A: u32 = 0;
const Y: u32 = 1;
const K: u32 = 2;
const Z: u32 = 3;
const M: u32 = 4;
const N: u32 = 5;
const Q: u32 = 6;
const R: u32 = 0;
const S: u32 = 1;
const U: u32 = 2;
const T: u32 = 3;
const P1: u32 = 7;
const P2: u32 = 8;

/// `trigger(X) → P1(X) ∨ P2(X)`, each disjunct `→ ∃role.filler`. The wedge branches only
/// after the Horn fixpoint is saturated, so the edge this generates arrives in a LATER
/// drain than every other fact — no pending event can mask a missing trigger.
fn late_exists(trigger: u32, role: Role, filler: u32) -> Vec<DlClause> {
    vec![
        horn(
            vec![Atom::Class(cls(trigger), X)],
            vec![Atom::Class(cls(P1), X), Atom::Class(cls(P2), X)],
        ),
        horn(
            vec![Atom::Class(cls(P1), X)],
            vec![Atom::Exists(role, cls(filler), X)],
        ),
        horn(
            vec![Atom::Class(cls(P2), X)],
            vec![Atom::Exists(role, cls(filler), X)],
        ),
    ]
}

fn cls(i: u32) -> ClassId {
    ClassId::new(i)
}
fn fwd(i: u32) -> Role {
    Role::named(RoleId::new(i))
}
fn inv(i: u32) -> Role {
    Role::inverse(RoleId::new(i))
}
fn horn(body: Vec<Atom>, head: Vec<Atom>) -> DlClause {
    DlClause { body, head }
}

/// `r(X,1) ∧ s(1,2) → u(X,2)` and `Y(X) ∧ u(X,1) → Q(X)`, `Y ⊓ Q → ⊥`.
fn chain_and_consumer() -> Vec<DlClause> {
    vec![
        horn(
            vec![Atom::Role(fwd(R), X, 1), Atom::Role(fwd(S), 1, 2)],
            vec![Atom::Role(fwd(U), X, 2)],
        ),
        horn(
            vec![Atom::Class(cls(Y), X), Atom::Role(fwd(U), X, 1)],
            vec![Atom::Class(cls(Q), X)],
        ),
        horn(vec![Atom::Class(cls(Y), X), Atom::Class(cls(Q), X)], vec![]),
    ]
}

#[test]
fn control_forward_root_is_woken() {
    // Root A —r→ m, then m —s→ z arrives later (M → ∃s.Z): the back-trigger fires at
    // m's predecessor, the root, so u(root,z) is derived ⇒ Q ⇒ clash with Y.
    let mut cl = chain_and_consumer();
    cl.push(horn(
        vec![Atom::Class(cls(A), X)],
        vec![Atom::Class(cls(Y), X)],
    ));
    cl.push(horn(
        vec![Atom::Class(cls(A), X)],
        vec![Atom::Exists(fwd(R), cls(M), X)],
    ));
    cl.extend(late_exists(M, fwd(S), Z));
    let mut eng = HyperEngine::new(&cl, cls(A)).with_incremental_fixpoint();
    assert_eq!(eng.decide(64), HyperResult::Unsat);
}

#[test]
fn gap_a_root_behind_a_reverse_stored_edge() {
    // A → ∃r⁻.Y creates root —r⁻→ y, so r(y, root) holds with the edge stored at the
    // ROOT, i.e. `y` is a successor of the middle node, not a predecessor. Y → ∀r.K
    // puts K on the root (fired via #135's pos_first_target_trigger), and K → ∃s.Z
    // then adds root —s→ z. The chain clause is rooted at y: r(y,root) ∧ s(root,z).
    // The s-edge back-triggers at root.preds = ∅, so u(y,z) is never derived.
    let mut cl = chain_and_consumer();
    cl.push(horn(
        vec![Atom::Class(cls(A), X)],
        vec![Atom::Exists(inv(R), cls(Y), X)],
    ));
    cl.push(horn(
        vec![Atom::Class(cls(Y), X), Atom::Role(fwd(R), X, 1)],
        vec![Atom::Class(cls(K), 1)],
    ));
    cl.extend(late_exists(K, fwd(S), Z));
    // u(y,z) follows from r(y,root) ∧ s(root,z); Y ⊓ ∃u ⇒ Q clashes with Y.
    assert_gap_caught_only_by_the_net(&cl);
}

#[test]
fn gap_c_three_hop_body() {
    // A → ∃r.M, M → ∃s.N, N → ∃t.Z. Body r(X,1) ∧ s(1,2) ∧ t(2,3) → Q(X), rooted at the
    // root. The t-edge (last to arrive) back-triggers at n.preds = {m}, one level up —
    // never at the root.
    let mut cl = vec![
        horn(
            vec![Atom::Class(cls(A), X)],
            vec![Atom::Exists(fwd(R), cls(M), X)],
        ),
        horn(
            vec![Atom::Class(cls(M), X)],
            vec![Atom::Exists(fwd(S), cls(N), X)],
        ),
        horn(
            vec![
                Atom::Role(fwd(R), X, 1),
                Atom::Role(fwd(S), 1, 2),
                Atom::Role(fwd(T), 2, 3),
            ],
            vec![Atom::Class(cls(Q), X)],
        ),
        horn(vec![Atom::Class(cls(A), X), Atom::Class(cls(Q), X)], vec![]),
    ];
    cl.extend(late_exists(N, fwd(T), Z));
    assert_gap_caught_only_by_the_net(&cl);
}

/// `cl` is unsatisfiable. The incremental drain alone misses it (a KNOWN gap — if
/// this starts failing on the `Sat` line, the gap closed: flip it to `Unsat`), and
/// the #137 net recovers it, with and without edge pinning.
fn assert_gap_caught_only_by_the_net(cl: &[DlClause]) {
    for pin in [false, true] {
        assert_eq!(
            drain_only(cl, pin).decide(64),
            HyperResult::Sat,
            "pin={pin}: known trigger gap — the drain alone misses this clash"
        );
        let mut eng = HyperEngine::new(cl, cls(A))
            .with_incremental_fixpoint()
            .with_reseed_verify(true)
            .with_edge_pin(pin);
        assert_eq!(
            eng.decide(64),
            HyperResult::Unsat,
            "pin={pin}: the #137 net must catch it"
        );
    }
}

/// An engine on the incremental drain with the #137 net OFF, so a binding the edge
/// dispatch misses is visible as a wrong `Sat`.
fn drain_only(cl: &[DlClause], pin: bool) -> HyperEngine<'_> {
    HyperEngine::new(cl, cls(A))
        .with_incremental_fixpoint()
        .with_reseed_verify(false)
        .with_edge_pin(pin)
}

#[test]
fn pinned_back_trigger_keeps_a_non_first_leg() {
    // `role_back_trigger`: the edge is the SECOND leg, fired at the root above it.
    // Same clause set as `control_forward_root_is_woken`.
    let mut cl = chain_and_consumer();
    cl.push(horn(
        vec![Atom::Class(cls(A), X)],
        vec![Atom::Class(cls(Y), X)],
    ));
    cl.push(horn(
        vec![Atom::Class(cls(A), X)],
        vec![Atom::Exists(fwd(R), cls(M), X)],
    ));
    cl.extend(late_exists(M, fwd(S), Z));
    for pin in [false, true] {
        assert_eq!(
            drain_only(&cl, pin).decide(64),
            HyperResult::Unsat,
            "pin={pin}"
        );
    }
}

#[test]
fn pinned_inverse_first_leg_reads_the_edge_backwards() {
    // `inverse_first_trigger`: root —r→ d gives d an `r⁻`-successor. The clause
    // `r⁻(X,1) → Q(X)` is rooted at the edge TARGET, so the pin must take the
    // backward reading (u = tgt, v = src). `Q ⊓ M → ⊥` at d.
    let mut cl = vec![
        horn(vec![Atom::Role(inv(R), X, 1)], vec![Atom::Class(cls(Q), X)]),
        horn(vec![Atom::Class(cls(Q), X), Atom::Class(cls(M), X)], vec![]),
    ];
    cl.extend(late_exists(A, fwd(R), M));
    for pin in [false, true] {
        assert_eq!(
            drain_only(&cl, pin).decide(64),
            HyperResult::Unsat,
            "pin={pin}"
        );
    }
}

#[test]
fn pinned_positive_first_leg_at_an_inverse_edge_target() {
    // `pos_first_target_trigger` (#135): root —r⁻→ y means r(y, root). The clause
    // `r(X,1) → Q(X)` is rooted at y, the edge target, again a backward reading.
    let mut cl = vec![
        horn(vec![Atom::Role(fwd(R), X, 1)], vec![Atom::Class(cls(Q), X)]),
        horn(vec![Atom::Class(cls(Q), X), Atom::Class(cls(M), X)], vec![]),
    ];
    cl.extend(late_exists(A, inv(R), M));
    for pin in [false, true] {
        assert_eq!(
            drain_only(&cl, pin).decide(64),
            HyperResult::Unsat,
            "pin={pin}"
        );
    }
}

#[test]
fn pinned_two_hop_body_through_the_parent() {
    // A body whose X-rooted walk leaves the root through its PARENT (an inverse
    // atom) and comes back down: r⁻(X,1) ∧ s(1,2) → Q(X). The root's parent p
    // gets its s-edge late; the edge is the second leg, so the clause fires at
    // p's predecessors — and the root is p's successor, i.e. gap (a)'s shape. That
    // is not fixed here, so with the net off both arms must AGREE, whatever they
    // say: pinning must not change which bindings the dispatch can reach.
    let mut cl = vec![
        horn(
            vec![Atom::Class(cls(A), X)],
            vec![Atom::Exists(fwd(R), cls(M), X)],
        ),
        horn(
            vec![Atom::Role(inv(R), X, 1), Atom::Role(fwd(S), 1, 2)],
            vec![Atom::Class(cls(Q), X)],
        ),
        horn(vec![Atom::Class(cls(Q), X), Atom::Class(cls(M), X)], vec![]),
    ];
    cl.extend(late_exists(A, fwd(S), Z));
    let off = drain_only(&cl, false).decide(64);
    let on = drain_only(&cl, true).decide(64);
    assert_eq!(off, on);
    // And with the net on (default), the entailed clash is found either way.
    for pin in [false, true] {
        let mut eng = HyperEngine::new(&cl, cls(A))
            .with_incremental_fixpoint()
            .with_reseed_verify(true)
            .with_edge_pin(pin);
        assert_eq!(eng.decide(64), HyperResult::Unsat, "pin={pin}");
    }
}
