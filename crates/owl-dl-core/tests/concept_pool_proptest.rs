//! Property tests for the [`owl_dl_core::ir::ConceptPool`] interning invariant.
//!
//! These tests are differential against a deterministic recursive `realize`
//! function: any structural duplication or commutation of And/Or operands
//! must collapse to the same [`ConceptId`].

use owl_dl_core::ir::{ClassId, ConceptExpr, ConceptId, ConceptPool, Role, RoleId};
use proptest::prelude::*;

/// A user-level concept builder used only by the proptest. Resembles the IR
/// without forcing the caller to thread a pool through construction.
#[derive(Clone, Debug)]
enum Build {
    Top,
    Bot,
    Atomic(u8),
    Not(Box<Build>),
    And(Vec<Build>),
    Or(Vec<Build>),
    Some(u8, Box<Build>),
    All(u8, Box<Build>),
}

fn arb_build() -> impl Strategy<Value = Build> {
    let leaf = prop_oneof![
        Just(Build::Top),
        Just(Build::Bot),
        (0u8..8).prop_map(Build::Atomic),
    ];
    leaf.prop_recursive(4, 32, 4, |inner| {
        prop_oneof![
            inner.clone().prop_map(|b| Build::Not(Box::new(b))),
            prop::collection::vec(inner.clone(), 1..4).prop_map(Build::And),
            prop::collection::vec(inner.clone(), 1..4).prop_map(Build::Or),
            (0u8..4, inner.clone()).prop_map(|(r, b)| Build::Some(r, Box::new(b))),
            (0u8..4, inner).prop_map(|(r, b)| Build::All(r, Box::new(b))),
        ]
    })
}

fn realize(pool: &mut ConceptPool, b: &Build) -> ConceptId {
    match b {
        Build::Top => pool.top(),
        Build::Bot => pool.bot(),
        Build::Atomic(c) => pool.atomic(ClassId::new(u32::from(*c))),
        Build::Not(inner) => {
            let inner = realize(pool, inner);
            pool.not(inner)
        }
        Build::And(xs) => {
            let ids: Vec<ConceptId> = xs.iter().map(|x| realize(pool, x)).collect();
            pool.and(ids)
        }
        Build::Or(xs) => {
            let ids: Vec<ConceptId> = xs.iter().map(|x| realize(pool, x)).collect();
            pool.or(ids)
        }
        Build::Some(r, inner) => {
            let inner = realize(pool, inner);
            pool.some(Role::named(RoleId::new(u32::from(*r))), inner)
        }
        Build::All(r, inner) => {
            let inner = realize(pool, inner);
            pool.all(Role::named(RoleId::new(u32::from(*r))), inner)
        }
    }
}

proptest! {
    /// Interning is deterministic: the same `Build` realized twice in the
    /// same pool returns the same id.
    #[test]
    fn intern_is_deterministic(b in arb_build()) {
        let mut pool = ConceptPool::new();
        let id1 = realize(&mut pool, &b);
        let id2 = realize(&mut pool, &b);
        prop_assert_eq!(id1, id2);
    }

    /// Re-interning never grows the pool past its first-call size.
    #[test]
    fn intern_is_monotone_under_repetition(b in arb_build()) {
        let mut pool = ConceptPool::new();
        let _ = realize(&mut pool, &b);
        let snapshot = pool.len();
        for _ in 0..10 {
            let _ = realize(&mut pool, &b);
        }
        prop_assert_eq!(pool.len(), snapshot);
    }

    /// `get` returns an expression whose top-level constructor matches the
    /// `Build` we asked for.
    #[test]
    fn get_round_trips_top_level_shape(b in arb_build()) {
        let mut pool = ConceptPool::new();
        let id = realize(&mut pool, &b);
        let expr = pool.get(id).clone();
        match &b {
            Build::Top        => prop_assert!(matches!(expr, ConceptExpr::Top)),
            Build::Bot        => prop_assert!(matches!(expr, ConceptExpr::Bot)),
            Build::Atomic(_)  => prop_assert!(matches!(expr, ConceptExpr::Atomic(_))),
            Build::Not(_)     => prop_assert!(matches!(expr, ConceptExpr::Not(_))),
            Build::And(_)     => prop_assert!(matches!(expr, ConceptExpr::And(_))),
            Build::Or(_)      => prop_assert!(matches!(expr, ConceptExpr::Or(_))),
            Build::Some(_, _) => prop_assert!(matches!(expr, ConceptExpr::Some(_, _))),
            Build::All(_, _)  => prop_assert!(matches!(expr, ConceptExpr::All(_, _))),
        }
    }

    /// Reversing the operand order of an `And` does not produce a new id.
    #[test]
    fn and_is_commutative(xs in prop::collection::vec(0u8..16, 1..6)) {
        let mut pool = ConceptPool::new();
        let cs: Vec<ConceptId> = xs.iter().map(|&c| pool.atomic(ClassId::new(u32::from(c)))).collect();
        let id1 = pool.and(cs.iter().copied());
        let mut perm = cs.clone();
        perm.reverse();
        let id2 = pool.and(perm);
        prop_assert_eq!(id1, id2);
    }

    /// Reversing the operand order of an `Or` does not produce a new id.
    #[test]
    fn or_is_commutative(xs in prop::collection::vec(0u8..16, 1..6)) {
        let mut pool = ConceptPool::new();
        let cs: Vec<ConceptId> = xs.iter().map(|&c| pool.atomic(ClassId::new(u32::from(c)))).collect();
        let id1 = pool.or(cs.iter().copied());
        let mut perm = cs.clone();
        perm.reverse();
        let id2 = pool.or(perm);
        prop_assert_eq!(id1, id2);
    }

    /// Duplicated operands in an `And` are deduped: `And([xs ++ xs]) == And([xs])`.
    #[test]
    fn and_dedups_duplicate_operands(xs in prop::collection::vec(0u8..8, 1..6)) {
        let mut pool = ConceptPool::new();
        let cs_single: Vec<ConceptId> = xs.iter().map(|&c| pool.atomic(ClassId::new(u32::from(c)))).collect();
        let cs_doubled: Vec<ConceptId> = xs.iter().chain(xs.iter())
            .map(|&c| pool.atomic(ClassId::new(u32::from(c))))
            .collect();
        let id_single = pool.and(cs_single);
        let id_doubled = pool.and(cs_doubled);
        prop_assert_eq!(id_single, id_doubled);
    }
}
