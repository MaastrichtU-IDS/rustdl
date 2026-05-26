// Phase 1 scaffolding: the data structure ships ahead of its
// integration with `PreparedOntology::decide`. Unit tests
// exercise every method, but no production call site does yet.
// The full path lands in Phase 2 per `docs/model-caching-plan.md`.
#![allow(dead_code)]

//! Tableau model cache — Phase 1 scaffolding.
//!
//! See [`docs/model-caching-plan.md`](../../docs/model-caching-plan.md)
//! for the full design. This module ships the data structure and
//! soundness primitives; integration with [`crate::PreparedOntology`]
//! and the `decide` call site lands in Phase 2.
//!
//! ## Why this lives behind a Phase-1 stub
//!
//! Model caching is the lever that `HermiT` uses to close the
//! pizza-shape gap (see `docs/perf-2026-05-24-new-server.md` §6).
//! Wiring it in one commit risks shipping an unsound cache that
//! reports false sat-verdicts — exactly the failure mode the
//! [`docs/moms-plan.md`](../../docs/moms-plan.md) §A reverts
//! cautioned against. Splitting Phase 1 (data structure +
//! soundness predicates, no integration) lets the next session
//! land the integration with a clean review surface.

use dashmap::DashMap;
use std::sync::Arc;

use owl_dl_core::ConceptId;

/// A satisfying-model snapshot from a confirmed Sat verdict.
/// Stored under the test concept's "fixed half" (see plan §
/// "Cache key").
#[derive(Debug, Clone)]
pub(crate) struct CachedModel {
    /// Sorted+dedup labels asserted at the completion graph's
    /// root when the tableau reached Sat. Sorted order keeps
    /// the compatibility check at `O(|root_labels| + |extra|)`
    /// rather than `O(|root_labels| × |extra|)`.
    pub(crate) root_labels: Vec<ConceptId>,
}

impl CachedModel {
    /// Snapshot a satisfying model from its root labels. Caller
    /// is responsible for passing labels from a node that the
    /// tableau just confirmed clash-free.
    #[must_use]
    pub(crate) fn from_root_labels(mut labels: Vec<ConceptId>) -> Self {
        labels.sort_unstable();
        labels.dedup();
        Self {
            root_labels: labels,
        }
    }

    /// Conservative compatibility check: returns `true` iff
    /// adding `extra` to this cached model definitely cannot
    /// produce a clash via simple label-vs-complement
    /// inspection.
    ///
    /// The check is sound under-approximation in the
    /// cache-hit direction:
    /// - returning `true` means "no obvious clash; the cached
    ///   model + extra is a witness for `key ⊓ extra`."
    /// - returning `false` means "either we found a label-vs-
    ///   complement collision, or we can't cheaply prove no
    ///   clash exists." The caller must fall through to a full
    ///   tableau probe.
    ///
    /// `complement` is the precomputed NNF complement of
    /// `extra` (the caller has it available via
    /// `PreparedOntology::complements`). Passing the wrong
    /// complement breaks soundness — keep the lookup co-located
    /// with the call site.
    #[must_use]
    pub(crate) fn is_compatible_with(
        &self,
        extra: ConceptId,
        complement: Option<ConceptId>,
    ) -> bool {
        // Direct collision: extra's complement already labelled
        // at the root ⇒ guaranteed clash on insertion.
        if let Some(neg) = complement
            && self.root_labels.binary_search(&neg).is_ok()
        {
            return false;
        }
        // Direct redundancy: extra is already labelled. The cached
        // model is already a witness for `key ⊓ extra`.
        if self.root_labels.binary_search(&extra).is_ok() {
            return true;
        }
        // Otherwise: we don't have enough info to rule out a
        // deeper clash via rule propagation. Return false so the
        // caller falls through to a full tableau probe.
        //
        // Phase 3 widens this to replay `apply_and` and
        // `apply_concept_rules` against the snapshot before
        // declaring compatibility — see plan §
        // "Phase 3 (later session)".
        false
    }
}

/// Thread-safe map of `ConceptId → CachedModel`. The `Arc<DashMap>`
/// shape lets the classify-pair-loop's rayon workers share the
/// cache without explicit locking; reads are lock-free, writes
/// are bucket-locked.
#[derive(Clone, Debug, Default)]
pub(crate) struct ModelCache {
    inner: Arc<DashMap<ConceptId, CachedModel>>,
}

impl ModelCache {
    #[must_use]
    pub(crate) fn new() -> Self {
        Self {
            inner: Arc::new(DashMap::new()),
        }
    }

    /// Return the cached model for `key`, if any. Borrowed
    /// through `DashMap`'s per-bucket lock for the duration of the
    /// returned guard; clone the inner labels into an owned vec
    /// before releasing if the caller needs to outlive the
    /// guard.
    pub(crate) fn get_cloned(&self, key: ConceptId) -> Option<CachedModel> {
        self.inner.get(&key).map(|r| r.clone())
    }

    /// Store a model for `key`. Overwrites any prior entry —
    /// the latest Sat verdict is the freshest witness.
    pub(crate) fn insert(&self, key: ConceptId, model: CachedModel) {
        self.inner.insert(key, model);
    }

    #[must_use]
    pub(crate) fn len(&self) -> usize {
        self.inner.len()
    }

    #[must_use]
    pub(crate) fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use owl_dl_core::ConceptId;

    #[test]
    fn cached_model_normalises_labels() {
        // Labels passed unsorted-with-duplicates should be
        // canonicalised so the binary_search-based compatibility
        // checks are correct.
        let m = CachedModel::from_root_labels(vec![
            ConceptId::new(3),
            ConceptId::new(1),
            ConceptId::new(1),
            ConceptId::new(2),
        ]);
        assert_eq!(
            m.root_labels,
            vec![ConceptId::new(1), ConceptId::new(2), ConceptId::new(3)]
        );
    }

    #[test]
    fn compatible_when_extra_already_labelled() {
        // The cached model already has `extra` — adding it is
        // a no-op, so the model is a witness for `key ⊓ extra`.
        let m = CachedModel::from_root_labels(vec![ConceptId::new(5), ConceptId::new(7)]);
        assert!(m.is_compatible_with(ConceptId::new(5), Some(ConceptId::new(50))));
    }

    #[test]
    fn incompatible_when_complement_present() {
        // Extra = c, complement of c = c'. If c' is already a
        // label, asserting c clashes immediately.
        let m = CachedModel::from_root_labels(vec![
            ConceptId::new(2),
            ConceptId::new(4), // complement of `5`
        ]);
        assert!(!m.is_compatible_with(ConceptId::new(5), Some(ConceptId::new(4))));
    }

    #[test]
    fn falls_through_when_no_signal_either_way() {
        // Extra is novel; its complement is not in the labels.
        // Phase 1's check is conservative — return `false` so
        // the caller falls through to a full tableau probe.
        let m = CachedModel::from_root_labels(vec![ConceptId::new(1), ConceptId::new(2)]);
        assert!(!m.is_compatible_with(ConceptId::new(99), Some(ConceptId::new(98))));
    }

    #[test]
    fn cache_round_trip() {
        let cache = ModelCache::new();
        assert!(cache.is_empty());
        let key = ConceptId::new(42);
        let model = CachedModel::from_root_labels(vec![ConceptId::new(1)]);
        cache.insert(key, model.clone());
        assert_eq!(cache.len(), 1);
        let got = cache.get_cloned(key).expect("present after insert");
        assert_eq!(got.root_labels, model.root_labels);
    }
}
