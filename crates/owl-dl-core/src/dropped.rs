//! [`DroppedAxioms`] — a diagnostic tally of axioms silently dropped during
//! conversion (issue #43).
//!
//! Dropping an unsupported axiom is a SOUND under-approximation (a weaker KB
//! can only miss entailments, never fabricate one), but a caller with no
//! visibility into what was dropped can't tell a fully-supported ontology
//! from one that quietly lost content. This type is threaded through
//! [`crate::ontology::InternalOntology`] so [`crate::convert::convert_ontology`]
//! can record every drop instead of aborting or staying silent.

use std::collections::BTreeMap;

/// A tally of dropped axioms, keyed by a stable diagnostic label (e.g.
/// `"SubClassOf: anonymous individual"`). `BTreeMap` keeps iteration order
/// deterministic (load-bearing: `convert_ontology` promises reproducible
/// output across runs).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DroppedAxioms {
    by_kind: BTreeMap<String, u64>,
}

impl DroppedAxioms {
    /// Whether nothing was dropped.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_kind.is_empty()
    }

    /// Total count of dropped axioms across all kinds.
    #[must_use]
    pub fn total(&self) -> u64 {
        self.by_kind.values().sum()
    }

    /// Per-kind drop counts.
    #[must_use]
    pub fn by_kind(&self) -> &BTreeMap<String, u64> {
        &self.by_kind
    }

    /// Record one dropped axiom of the given diagnostic `kind`.
    pub fn record(&mut self, kind: String) {
        *self.by_kind.entry(kind).or_insert(0) += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_empty() {
        let d = DroppedAxioms::default();
        assert!(d.is_empty());
        assert_eq!(d.total(), 0);
        assert!(d.by_kind().is_empty());
    }

    #[test]
    fn record_tallies_by_kind() {
        let mut d = DroppedAxioms::default();
        d.record("SubClassOf: anonymous individual".to_string());
        d.record("SubClassOf: anonymous individual".to_string());
        d.record("ClassAssertion: unsupported data range".to_string());
        assert!(!d.is_empty());
        assert_eq!(d.total(), 3);
        assert_eq!(
            d.by_kind().get("SubClassOf: anonymous individual").copied(),
            Some(2)
        );
        assert_eq!(
            d.by_kind()
                .get("ClassAssertion: unsupported data range")
                .copied(),
            Some(1)
        );
    }
}
