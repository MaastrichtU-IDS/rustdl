//! IR for OWL DL concepts, roles, and identifiers.
//!
//! Concept expressions are interned in a [`ConceptPool`] for structural
//! sharing: every logically equivalent sub-expression resolves to one
//! [`ConceptId`], so equality is O(1) integer comparison. This invariant is
//! load-bearing for the tableau hot loop and is established in Phase 0.
//!
//! Data-range concept constructors (`DataSome`, `DataAll`, `DataMin`,
//! `DataMax`) land in Phase 3 alongside the minimal datatype slice.

use hashbrown::HashMap;

/// Index of a named class (e.g. `Person`). Interning of class IRIs to
/// [`ClassId`]s lives outside this module — see the upcoming vocabulary
/// type planned for Day 9-12.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Ord, PartialOrd, Debug)]
pub struct ClassId(u32);

impl ClassId {
    #[must_use]
    pub const fn new(idx: u32) -> Self {
        Self(idx)
    }

    #[must_use]
    pub const fn index(self) -> u32 {
        self.0
    }
}

/// Index of a named object property (role).
#[derive(Copy, Clone, Eq, PartialEq, Hash, Ord, PartialOrd, Debug)]
pub struct RoleId(u32);

impl RoleId {
    #[must_use]
    pub const fn new(idx: u32) -> Self {
        Self(idx)
    }

    #[must_use]
    pub const fn index(self) -> u32 {
        self.0
    }
}

/// Index of a named individual.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Ord, PartialOrd, Debug)]
pub struct IndividualId(u32);

impl IndividualId {
    #[must_use]
    pub const fn new(idx: u32) -> Self {
        Self(idx)
    }

    #[must_use]
    pub const fn index(self) -> u32 {
        self.0
    }
}

/// A role expression as it appears in a concept (`∀R.C`, `∃R.C`, …)
/// or in an `RBox` axiom.
///
/// A role is either a *named* property (the common case) or the
/// *inverse* of a named property. Inverse roles are part of `ALCI`
/// onward; the constructor [`Role::inverse`] is exposed in Phase 3
/// commit 1's refactor pass but the converter still only produces
/// [`Role::Named`] until Phase 3 commit 2 wires up `ObjectInverseOf`.
///
/// Call sites that only need the underlying [`RoleId`] keep using
/// [`Role::role_id`]; sites that care about polarity check
/// [`Role::is_inverse`] or destructure.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Ord, PartialOrd, Debug)]
pub enum Role {
    Named(RoleId),
    Inverse(RoleId),
}

impl Role {
    #[must_use]
    pub const fn named(id: RoleId) -> Self {
        Self::Named(id)
    }

    #[must_use]
    pub const fn inverse(id: RoleId) -> Self {
        Self::Inverse(id)
    }

    /// The underlying named role, regardless of polarity. `r⁻` and
    /// `r` both report the same `role_id`; use [`Self::is_inverse`]
    /// to disambiguate.
    #[must_use]
    pub const fn role_id(self) -> RoleId {
        match self {
            Self::Named(id) | Self::Inverse(id) => id,
        }
    }

    #[must_use]
    pub const fn is_inverse(self) -> bool {
        matches!(self, Self::Inverse(_))
    }

    /// Flip polarity: `r` ↔ `r⁻`. Useful when traversing an edge
    /// "backwards" or applying ∃r⁻ as an ∃r at the predecessor.
    #[must_use]
    pub const fn flip(self) -> Self {
        match self {
            Self::Named(id) => Self::Inverse(id),
            Self::Inverse(id) => Self::Named(id),
        }
    }
}

/// Index of an interned [`ConceptExpr`] in a [`ConceptPool`].
///
/// Equality of `ConceptId`s from the same pool is O(1) integer comparison and
/// is iff equality on the underlying concept expressions (modulo the pool's
/// canonicalization of And/Or operand order).
#[derive(Copy, Clone, Eq, PartialEq, Hash, Ord, PartialOrd, Debug)]
pub struct ConceptId(u32);

impl ConceptId {
    /// Wrap a raw index into a `ConceptId`. The index must
    /// correspond to a `ConceptExpr` already interned in the
    /// pool this id will be used against; misuse yields panics
    /// or wrong-concept lookups, not unsafety. Mirrors
    /// [`ClassId::new`] / [`RoleId::new`] for callers (notably
    /// the tests in `crate::model_cache`) that need to mint ids
    /// without a pool round-trip.
    #[must_use]
    pub const fn new(idx: u32) -> Self {
        Self(idx)
    }

    #[must_use]
    pub const fn index(self) -> u32 {
        self.0
    }
}

/// An OWL DL concept expression. Sub-concepts are referenced by [`ConceptId`]
/// to preserve structural sharing.
///
/// Variant → DL syntax:
///
/// | variant                | DL              |
/// |------------------------|-----------------|
/// | `Top`                  | ⊤               |
/// | `Bot`                  | ⊥               |
/// | `Atomic(c)`            | named class C   |
/// | `Nominal(a)`           | {a}             |
/// | `SelfRestriction(r)`   | ∃r.Self         |
/// | `Not(c)`               | ¬C              |
/// | `And(cs)`              | C₁ ⊓ ... ⊓ Cₙ  (sorted + deduped) |
/// | `Or(cs)`               | C₁ ⊔ ... ⊔ Cₙ  (sorted + deduped) |
/// | `Some(r, c)`           | ∃r.C            |
/// | `All(r, c)`            | ∀r.C            |
/// | `Min(n, r, c)`         | ≥ n r.C         |
/// | `Max(n, r, c)`         | ≤ n r.C         |
#[derive(Clone, Eq, PartialEq, Hash, Debug)]
pub enum ConceptExpr {
    Top,
    Bot,
    Atomic(ClassId),
    Nominal(IndividualId),
    SelfRestriction(Role),
    Not(ConceptId),
    And(Box<[ConceptId]>),
    Or(Box<[ConceptId]>),
    Some(Role, ConceptId),
    All(Role, ConceptId),
    Min(u32, Role, ConceptId),
    Max(u32, Role, ConceptId),
}

/// Interning arena for [`ConceptExpr`].
///
/// Maintains a 1:1 bijection between distinct concept expressions and
/// [`ConceptId`]s. And/Or operand lists are sorted and deduped on intern so
/// that logically equivalent conjunctions and disjunctions hash to the same
/// `ConceptId` regardless of operand order or repetition.
#[derive(Default, Debug)]
pub struct ConceptPool {
    by_id: Vec<ConceptExpr>,
    by_expr: HashMap<ConceptExpr, ConceptId>,
    /// Phase 3c: cached `ConceptId` of `ConceptExpr::Bot`. Populated at most
    /// once (on the first `bot_id()` call that finds Bot in the pool). Bot is
    /// a unit variant interned at most once, so the id is stable forever once
    /// set. Uses `OnceLock` (not `Cell`) because `ConceptPool` is shared
    /// across rayon threads inside `PreparedOntology`. Eliminates the O(n)
    /// `iter_with_ids().find_map(...)` scan that the `apply_role_axioms`
    /// cluster attributed at 24.66% of post-Phase-3b classify cost. See
    /// `docs/superpowers/plans/2026-06-01-phase3c-bot-id-cache.md`.
    bot_id_cache: std::sync::OnceLock<ConceptId>,
    /// Phase 3c: per-call counter for `bot_id` cache hits. Bumped each
    /// time `bot_id()` returns the cached value without scanning. Used
    /// by the structural canary to confirm the cache is consulted.
    /// `AtomicU64` (not `Cell`) for `Sync` across rayon threads.
    bot_id_cache_hits: std::sync::atomic::AtomicU64,
}

impl Clone for ConceptPool {
    fn clone(&self) -> Self {
        Self {
            by_id: self.by_id.clone(),
            by_expr: self.by_expr.clone(),
            bot_id_cache: self.bot_id_cache.clone(),
            bot_id_cache_hits: std::sync::atomic::AtomicU64::new(
                self.bot_id_cache_hits
                    .load(std::sync::atomic::Ordering::Relaxed),
            ),
        }
    }
}

impl ConceptPool {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of distinct interned concept expressions.
    #[must_use]
    pub fn len(&self) -> usize {
        self.by_id.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }

    /// Look up the expression behind a [`ConceptId`].
    /// Iterate every interned expression in id order.
    pub fn iter_exprs(&self) -> impl Iterator<Item = &ConceptExpr> {
        self.by_id.iter()
    }

    /// Iterate every interned expression paired with its
    /// [`ConceptId`]. Useful for callers that need a stable handle.
    pub fn iter_with_ids(&self) -> impl Iterator<Item = (ConceptId, &ConceptExpr)> {
        self.by_id.iter().enumerate().map(|(i, e)| {
            (
                ConceptId(u32::try_from(i).expect("pool size fits in u32")),
                e,
            )
        })
    }

    /// Look up the canonical `⊥` id in the pool, if it has been
    /// interned. Returns `None` for empty pools or pools where no
    /// expression references `Bot`.
    #[must_use]
    pub fn bot_id(&self) -> Option<ConceptId> {
        // Phase 3c: cache-or-scan. Bot is a unit variant interned at
        // most once; once set in the OnceLock, the id is stable forever
        // (the pool doesn't re-intern or remove). If the cache is not
        // yet populated, we scan; if the scan finds Bot, we set the
        // OnceLock (subsequent calls hit). If the scan returns None
        // (Bot not yet interned), the lock stays empty — a later call
        // after interning Bot will scan again, find it, and populate then.
        // OnceLock (not Cell) because ConceptPool is shared across rayon
        // threads inside PreparedOntology.
        if let Some(&cached) = self.bot_id_cache.get() {
            self.bot_id_cache_hits
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            return Some(cached);
        }
        let found = self.iter_with_ids().find_map(|(id, e)| {
            if matches!(e, ConceptExpr::Bot) {
                Some(id)
            } else {
                None
            }
        });
        if let Some(id) = found {
            // Ignore the error: a concurrent thread may have set it
            // first (same id, idempotent).
            let _ = self.bot_id_cache.set(id);
        }
        found
    }

    /// Phase 3c: read the `bot_id` cache-hit counter (test-facing).
    #[must_use]
    pub fn bot_id_cache_hits(&self) -> u64 {
        self.bot_id_cache_hits
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    ///
    /// # Panics
    /// Panics if `id` was not produced by this pool.
    #[must_use]
    pub fn get(&self, id: ConceptId) -> &ConceptExpr {
        &self.by_id[id.0 as usize]
    }

    fn intern_raw(&mut self, expr: ConceptExpr) -> ConceptId {
        if let Some(&id) = self.by_expr.get(&expr) {
            return id;
        }
        let id =
            ConceptId(u32::try_from(self.by_id.len()).expect("ConceptPool size exceeds u32::MAX"));
        self.by_expr.insert(expr.clone(), id);
        self.by_id.push(expr);
        id
    }

    pub fn top(&mut self) -> ConceptId {
        self.intern_raw(ConceptExpr::Top)
    }

    pub fn bot(&mut self) -> ConceptId {
        self.intern_raw(ConceptExpr::Bot)
    }

    pub fn atomic(&mut self, c: ClassId) -> ConceptId {
        self.intern_raw(ConceptExpr::Atomic(c))
    }

    pub fn nominal(&mut self, i: IndividualId) -> ConceptId {
        self.intern_raw(ConceptExpr::Nominal(i))
    }

    pub fn self_restriction(&mut self, r: Role) -> ConceptId {
        self.intern_raw(ConceptExpr::SelfRestriction(r))
    }

    pub fn not(&mut self, c: ConceptId) -> ConceptId {
        self.intern_raw(ConceptExpr::Not(c))
    }

    pub fn some(&mut self, r: Role, c: ConceptId) -> ConceptId {
        self.intern_raw(ConceptExpr::Some(r, c))
    }

    pub fn all(&mut self, r: Role, c: ConceptId) -> ConceptId {
        self.intern_raw(ConceptExpr::All(r, c))
    }

    /// Intern `≥n r.C`. Folds trivial cases at intern time:
    /// `≥0 r.C ≡ ⊤` (every individual satisfies "at least zero").
    pub fn min(&mut self, n: u32, r: Role, c: ConceptId) -> ConceptId {
        if n == 0 {
            return self.top();
        }
        self.intern_raw(ConceptExpr::Min(n, r, c))
    }

    pub fn max(&mut self, n: u32, r: Role, c: ConceptId) -> ConceptId {
        self.intern_raw(ConceptExpr::Max(n, r, c))
    }

    /// Intern an And with the following Boolean normalizations applied:
    ///
    /// - **Flatten nested Ands**: `And([And([a, b]), c])` → `And([a, b, c])`.
    /// - **Drop Top** (the identity of And): `And([a, ⊤, b])` → `And([a, b])`.
    /// - **Short-circuit on Bot** (the annihilator): `And([a, ⊥, b])` → `⊥`.
    /// - **Sort + dedup** operands so commutative-equivalent expressions
    ///   collide.
    /// - **Collapse**: empty → `⊤`; single → the operand.
    pub fn and(&mut self, args: impl IntoIterator<Item = ConceptId>) -> ConceptId {
        let mut v: Vec<ConceptId> = Vec::new();
        let mut any_bot = false;
        for arg in args {
            match self.get(arg) {
                ConceptExpr::Top => {} // identity — skip
                ConceptExpr::Bot => any_bot = true,
                ConceptExpr::And(inner) => v.extend_from_slice(inner),
                _ => v.push(arg),
            }
        }
        if any_bot {
            return self.bot();
        }
        v.sort_unstable();
        v.dedup();
        if v.is_empty() {
            return self.top();
        }
        if v.len() == 1 {
            return v[0];
        }
        self.intern_raw(ConceptExpr::And(v.into_boxed_slice()))
    }

    /// Intern an Or with the dual normalizations:
    ///
    /// - **Flatten nested Ors**: `Or([Or([a, b]), c])` → `Or([a, b, c])`.
    /// - **Drop Bot** (the identity of Or): `Or([a, ⊥, b])` → `Or([a, b])`.
    /// - **Short-circuit on Top** (the annihilator): `Or([a, ⊤, b])` → `⊤`.
    /// - **Sort + dedup** operands.
    /// - **Collapse**: empty → `⊥`; single → the operand.
    pub fn or(&mut self, args: impl IntoIterator<Item = ConceptId>) -> ConceptId {
        let mut v: Vec<ConceptId> = Vec::new();
        let mut any_top = false;
        for arg in args {
            match self.get(arg) {
                ConceptExpr::Bot => {} // identity — skip
                ConceptExpr::Top => any_top = true,
                ConceptExpr::Or(inner) => v.extend_from_slice(inner),
                _ => v.push(arg),
            }
        }
        if any_top {
            return self.top();
        }
        v.sort_unstable();
        v.dedup();
        if v.is_empty() {
            return self.bot();
        }
        if v.len() == 1 {
            return v[0];
        }
        self.intern_raw(ConceptExpr::Or(v.into_boxed_slice()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intern_dedups_atomics() {
        let mut pool = ConceptPool::new();
        let a1 = pool.atomic(ClassId::new(0));
        let a2 = pool.atomic(ClassId::new(0));
        assert_eq!(a1, a2);
        assert_eq!(pool.len(), 1);
    }

    #[test]
    fn distinct_atomic_ids_distinct() {
        let mut pool = ConceptPool::new();
        let a = pool.atomic(ClassId::new(0));
        let b = pool.atomic(ClassId::new(1));
        assert_ne!(a, b);
        assert_eq!(pool.len(), 2);
    }

    #[test]
    fn and_is_commutative() {
        let mut pool = ConceptPool::new();
        let a = pool.atomic(ClassId::new(0));
        let b = pool.atomic(ClassId::new(1));
        let ab = pool.and([a, b]);
        let ba = pool.and([b, a]);
        assert_eq!(ab, ba);
        // Distinct interned exprs: A, B, A⊓B
        assert_eq!(pool.len(), 3);
    }

    #[test]
    fn and_dedups_duplicate_operands() {
        let mut pool = ConceptPool::new();
        let a = pool.atomic(ClassId::new(0));
        let id1 = pool.and([a, a, a]);
        let id2 = pool.and([a]);
        assert_eq!(id1, id2);
    }

    #[test]
    fn shared_sub_concepts_share_ids() {
        let mut pool = ConceptPool::new();
        let r = Role::named(RoleId::new(0));
        let a = pool.atomic(ClassId::new(0));
        let s1 = pool.some(r, a);
        let s2 = pool.some(r, a);
        assert_eq!(s1, s2);
        // Distinct interned exprs: A, ∃r.A
        assert_eq!(pool.len(), 2);
    }

    #[test]
    fn get_returns_interned_expr() {
        let mut pool = ConceptPool::new();
        let a = pool.atomic(ClassId::new(7));
        match pool.get(a) {
            ConceptExpr::Atomic(c) => assert_eq!(*c, ClassId::new(7)),
            other => panic!("expected Atomic, got {other:?}"),
        }
    }

    #[test]
    fn role_round_trip() {
        let r = Role::named(RoleId::new(42));
        assert_eq!(r.role_id(), RoleId::new(42));
        assert_eq!(r.role_id().index(), 42);
    }

    // ── Boolean-normalization tests for and/or ────────────────────────

    #[test]
    fn and_flattens_nested_and() {
        let mut p = ConceptPool::new();
        let a = p.atomic(ClassId::new(0));
        let b = p.atomic(ClassId::new(1));
        let c = p.atomic(ClassId::new(2));
        let inner = p.and([a, b]);
        let outer = p.and([inner, c]);
        // outer = And([a, b, c]), not And([And([a, b]), c]).
        match p.get(outer) {
            ConceptExpr::And(args) => assert_eq!(args.len(), 3),
            other => panic!("expected flat And, got {other:?}"),
        }
    }

    #[test]
    fn and_drops_top() {
        let mut p = ConceptPool::new();
        let a = p.atomic(ClassId::new(0));
        let t = p.top();
        let result = p.and([a, t]);
        // And([a, Top]) = a.
        assert_eq!(result, a);
    }

    #[test]
    fn and_with_bot_collapses_to_bot() {
        let mut p = ConceptPool::new();
        let a = p.atomic(ClassId::new(0));
        let b = p.bot();
        let result = p.and([a, b]);
        assert_eq!(result, b);
    }

    #[test]
    fn empty_and_is_top() {
        let mut p = ConceptPool::new();
        let empty: Vec<ConceptId> = Vec::new();
        let result = p.and(empty);
        assert_eq!(result, p.top());
    }

    #[test]
    fn or_flattens_nested_or() {
        let mut p = ConceptPool::new();
        let a = p.atomic(ClassId::new(0));
        let b = p.atomic(ClassId::new(1));
        let c = p.atomic(ClassId::new(2));
        let inner = p.or([a, b]);
        let outer = p.or([inner, c]);
        match p.get(outer) {
            ConceptExpr::Or(args) => assert_eq!(args.len(), 3),
            other => panic!("expected flat Or, got {other:?}"),
        }
    }

    #[test]
    fn or_drops_bot() {
        let mut p = ConceptPool::new();
        let a = p.atomic(ClassId::new(0));
        let b = p.bot();
        let result = p.or([a, b]);
        assert_eq!(result, a);
    }

    #[test]
    fn or_with_top_collapses_to_top() {
        let mut p = ConceptPool::new();
        let a = p.atomic(ClassId::new(0));
        let t = p.top();
        let result = p.or([a, t]);
        assert_eq!(result, t);
    }

    #[test]
    fn empty_or_is_bot() {
        let mut p = ConceptPool::new();
        let empty: Vec<ConceptId> = Vec::new();
        let result = p.or(empty);
        assert_eq!(result, p.bot());
    }

    // ── Phase 3c canaries ─────────────────────────────────────────────────
    // Asserts semantic equivalence of bot_id() before and after cache
    // population. Pre-fix: every call does a linear scan. Post-fix: first
    // call scans + caches; subsequent calls hit the cache. The returned
    // ConceptId (or None) must be identical either way.
    #[test]
    fn phase3c_bot_id_returns_same_before_and_after_cache_population() {
        let mut pool = ConceptPool::new();

        // Before Bot is interned, bot_id() must return None — stably.
        let first = pool.bot_id();
        // Intern some non-Bot concepts so the pool has multiple entries
        // and the linear scan must walk past them.
        let c0 = pool.atomic(ClassId::new(0));
        let c1 = pool.atomic(ClassId::new(1));
        let _and = pool.and([c0, c1]);
        let second = pool.bot_id();
        assert_eq!(
            first, second,
            "bot_id() before Bot interning must be stable across pool growth"
        );
        assert!(first.is_none(), "bot_id() returns None before Bot is interned");

        // Intern Bot. Both subsequent calls must return the same Some(id).
        let _bot_id = pool.bot();
        let third = pool.bot_id();
        let fourth = pool.bot_id();
        assert!(third.is_some(), "after pool.bot(), bot_id() must be Some");
        assert_eq!(
            third, fourth,
            "subsequent bot_id() calls must return the same id"
        );
    }

    #[test]
    fn phase3c_bot_id_cache_hits_counter_bumps_on_repeat_calls() {
        let mut pool = ConceptPool::new();
        let _ = pool.bot(); // intern Bot so the cache will populate
        let _ = pool.bot_id(); // first call: scans + populates
        let before = pool.bot_id_cache_hits();
        let _ = pool.bot_id();
        let _ = pool.bot_id();
        let _ = pool.bot_id();
        let after = pool.bot_id_cache_hits();
        assert!(
            after >= before + 3,
            "bot_id_cache_hits should increment on cached calls; \
             before={before} after={after}"
        );
    }
}
