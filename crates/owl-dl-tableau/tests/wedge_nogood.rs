//! SP2 B0: wedge-native node-local UNSAT oracle.
//!
//! `HyperEngine::node_local_closure` / `node_local_unsat` compute the
//! node-local forward-closure of a label-set over `self.clauses` +
//! `self.disjoint_pairs` — role-free, same-variable, non-disjunctive
//! clauses only, so `clashed` under-approximates the real engine's
//! node-local derivation (no false UNSAT).
//!
//! Fixture: `A ⊑ B` (`Class(A,X) → Class(B,X)`) and `B ⊓ C ⊑ ⊥`
//! (`Class(B,X) ∧ Class(C,X) → ⊥`). The crucial case is `{A, C}`: the
//! oracle must fire `A → B` first, then see the `B ⊓ C → ⊥` clash — a
//! multi-step, non-syntactic core (not a told-disjoint pair).

#![allow(clippy::unwrap_used)]
#![allow(clippy::many_single_char_names)]

use owl_dl_core::clause::{Atom, DlClause, X};
use owl_dl_core::ir::ClassId;
use owl_dl_tableau::hyper::HyperEngine;

fn cls(i: u32) -> ClassId {
    ClassId::new(i)
}

#[test]
fn node_local_unsat_multistep_core() {
    let (a, b, c, d, e) = (cls(0), cls(1), cls(2), cls(3), cls(4));

    let clauses = vec![
        // A ⊑ B
        DlClause {
            body: vec![Atom::Class(a, X)],
            head: vec![Atom::Class(b, X)],
        },
        // B ⊓ C ⊑ ⊥
        DlClause {
            body: vec![Atom::Class(b, X), Atom::Class(c, X)],
            head: vec![],
        },
    ];

    // root label is irrelevant here — we pass label-sets explicitly.
    let eng = HyperEngine::new(&clauses, a);

    // The multi-step core: A → B, then B ⊓ C → ⊥.
    assert!(
        eng.node_local_unsat(&[a, c]),
        "{{A,C}} must be UNSAT via A→B then B⊓C→⊥ (multi-step, non-syntactic core)"
    );
    // A alone derives B but nothing clashes.
    assert!(!eng.node_local_unsat(&[a]), "{{A}} must be satisfiable");
    // Unrelated labels — no clause fires.
    assert!(
        !eng.node_local_unsat(&[d, e]),
        "{{D,E}} must be satisfiable"
    );
}
