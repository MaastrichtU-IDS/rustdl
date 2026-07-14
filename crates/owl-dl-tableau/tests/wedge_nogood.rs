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
//! oracle must fire `A → B` first, then reach the clash — a multi-step
//! derivation exercising single-Class-head insertion. (That `B ⊓ C → ⊥`
//! clause IS a 2-atom-same-var ⊥ body, so `build_disjoint_pairs` also
//! captures it into `disjoint_pairs`; the actual clash there fires via
//! the disjoint-pairs branch.)
//!
//! A separate 3-atom ⊥-headed clause `A ⊓ B ⊓ C → ⊥` exercises the
//! `head.is_empty()` branch in isolation: `build_disjoint_pairs` requires
//! `body.len() == 2`, so this clause is NOT in `disjoint_pairs` and the
//! empty-head branch is the only thing that can detect its clash.

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

    // The multi-step core: A → B, then B ⊓ C clashes (via disjoint_pairs, since
    // the 2-same-var ⊥ body is a told-disjoint pair). Validates the A→B
    // single-Class-head insertion feeding a downstream clash.
    assert!(
        eng.node_local_unsat(&[a, c]),
        "{{A,C}} must be UNSAT via A→B then the B⊓C clash (multi-step)"
    );
    // A alone derives B but nothing clashes.
    assert!(!eng.node_local_unsat(&[a]), "{{A}} must be satisfiable");
    // Unrelated labels — no clause fires.
    assert!(
        !eng.node_local_unsat(&[d, e]),
        "{{D,E}} must be satisfiable"
    );
}

/// Exercises the `head.is_empty()` branch in ISOLATION: a 3-atom ⊥-headed
/// clause `A ⊓ B ⊓ C → ⊥` is NOT a told-disjoint pair (`build_disjoint_pairs`
/// needs `body.len() == 2`), so only the empty-head branch can detect it.
#[test]
fn node_local_unsat_nonbinary_bottom_head() {
    let (a, b, c) = (cls(0), cls(1), cls(2));

    let clauses = vec![
        // A ⊓ B ⊓ C ⊑ ⊥ — 3-atom body ⇒ NOT captured into disjoint_pairs.
        DlClause {
            body: vec![Atom::Class(a, X), Atom::Class(b, X), Atom::Class(c, X)],
            head: vec![],
        },
    ];

    let eng = HyperEngine::new(&clauses, a);

    // All three present ⇒ empty-head clash (the ONLY detection path here).
    assert!(
        eng.node_local_unsat(&[a, b, c]),
        "{{A,B,C}} must be UNSAT via the head.is_empty() branch (not a disjoint pair)"
    );
    // Only two of three ⇒ body does not match ⇒ no clash.
    assert!(
        !eng.node_local_unsat(&[a, b]),
        "{{A,B}} must be satisfiable (3-atom body under-satisfied)"
    );
}

/// SP2 B1 KEY assertion: on `A⊑B, B⊓C⊑⊥` with node labels `{A,B,C,…}` where
/// `B` is a *derived* label (its dep-set is inherited from `A`), extraction
/// must recover the **antecedent** core `{A,C}` — NOT the derived told-disjoint
/// pair `{B,C}` (which the disjoint-pair filter would collapse to `None`), and
/// NOT `None`. The re-derivability filter (step 3) is what drops `B`.
///
/// Driven via the explicit-dep-level adapter (`extract_core_from_dep_levels`)
/// so the pool/filter/minimize logic is exercised in isolation; the live-clash
/// wiring (real `HNode`/`DepSet` from an engine clash) lands in B3.
#[test]
fn extract_core_recovers_antecedent_not_derived_pair() {
    let (a, b, c) = (cls(0), cls(1), cls(2));
    let e = cls(3); // dep-disjoint noise: its dep does NOT intersect body_deps ⇒ excluded from pool.
    let noise = cls(5); // empty-dep noise: enters the pool (seed/root-given) but is minimized away.

    let clauses = vec![
        // A ⊑ B
        DlClause {
            body: vec![Atom::Class(a, X)],
            head: vec![Atom::Class(b, X)],
        },
        // B ⊓ C ⊑ ⊥  (also a told-disjoint pair (B,C))
        DlClause {
            body: vec![Atom::Class(b, X), Atom::Class(c, X)],
            head: vec![],
        },
    ];
    let eng = HyperEngine::new(&clauses, a);

    // Decision levels: dA = 1, dC = 2, dOther = 3.
    // A carries dA; B is DERIVED from A ⇒ inherits {dA}; C carries dC;
    // E carries dOther (does not intersect body_deps); noise is seed-given (empty).
    let labels = [a, b, c, e, noise];
    let label_dep_levels = vec![
        vec![1u32], // A
        vec![1u32], // B (derived from A — inherits A's deps)
        vec![2u32], // C
        vec![3u32], // E (dep-disjoint from body_deps)
        vec![],     // noise (empty deps)
    ];
    let body_dep_levels = [1u32, 2]; // clash body_deps = {dA, dC}

    let core = eng.extract_core_from_dep_levels(&labels, &label_dep_levels, &body_dep_levels);
    assert_eq!(
        core,
        Some(vec![a, c]),
        "must recover antecedent {{A,C}} (B re-derivable ⇒ dropped; E dep-disjoint ⇒ excluded; noise minimized away) — NOT {{B,C}}, NOT None"
    );
}

/// A clash whose minimal core is exactly a told-disjoint pair `(B,C)` must
/// return `None` (clause firing catches it eagerly; caching prunes nothing).
#[test]
fn extract_core_none_on_told_disjoint_pair() {
    let (b, c) = (cls(1), cls(2));
    let clauses = vec![
        // B ⊓ C ⊑ ⊥  ⇒ told-disjoint pair (B,C).
        DlClause {
            body: vec![Atom::Class(b, X), Atom::Class(c, X)],
            head: vec![],
        },
    ];
    let eng = HyperEngine::new(&clauses, b);

    let labels = [b, c];
    let label_dep_levels = vec![vec![1u32], vec![2u32]];
    let core = eng.extract_core_from_dep_levels(&labels, &label_dep_levels, &[1u32, 2]);
    assert_eq!(
        core, None,
        "a told-disjoint-pair-only core (B,C) must be filtered to None"
    );
}
