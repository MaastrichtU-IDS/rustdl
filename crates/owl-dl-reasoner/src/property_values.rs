//! Inferred `inferred_object_property_values` / `inferred_data_property_values`
//! queries (issue #45).
//!
//! **Object values**: sound seed (`materialize_object_property_assertions`,
//! already a sound lower bound over named individuals — asserted + sub-property
//! / inverse / symmetric / role-chain / transitive closure) plus a BUDGETED,
//! HARD-BOUNDED entailment extension. The extension deliberately does NOT
//! enumerate the full `|individuals|² × |properties|` cross-product — see
//! [`candidate_extension_pairs`] for the exact bounded policy. Each extension
//! candidate is checked via `PreparedOntology::consistent_with_extra`'s
//! `extra_neg_prop` path (Task 0.3): `R(a,b)` is entailed iff
//! `KB ∪ {¬R(a,b)}` is inconsistent — sound by construction (never derived
//! from a satisfying model).
//!
//! **Data values**: structural passthrough only (v1 boundary) —
//! `materialize_data_property_assertions`'s 5-tuples with `lang` dropped to a
//! 4-tuple. No entailment probe; `incomplete` is always `false` (complete for
//! that structural fragment, same as `materialize_data_property_assertions`
//! itself).
use crate::{PreparedOntology, ReasonError};
use horned_owl::model::ForIRI;
use horned_owl::ontology::set::SetOntology;
use owl_dl_core::convert::convert_ontology;
use owl_dl_core::ir::{IndividualId, RoleId};
use owl_dl_core::ontology::Axiom;
use std::collections::HashSet;
use std::time::{Duration, Instant};

/// Entailed OBJECT property triples over named individuals, plus a
/// completeness flag.
#[derive(Debug, Clone)]
pub struct ObjectPropertyValues {
    triples: Vec<(String, String, String)>,
    incomplete: bool,
}

impl ObjectPropertyValues {
    /// `(subject_iri, property_iri, object_iri)` triples, sorted and
    /// deduplicated.
    #[must_use]
    pub fn triples(&self) -> &[(String, String, String)] {
        &self.triples
    }

    /// `true` iff: the `TBox` lies outside the fragment
    /// (`analyze_fragment`'s `PureEl`/`Horn`) where the Horn-only seed is
    /// PROVABLY complete for every entailed edge over named individuals
    /// (e.g. a disjunctive `C ⊑ ∃R.{b} ⊔ ∃R.{c})` — the seed may then be
    /// missing edges the bounded extension has no candidate pair to even
    /// probe); OR a bounded-extension probe timed out (`None`); OR the
    /// bounded (non-exhaustive) extension ran at all. Object values beyond
    /// the seed neighborhood — i.e. entailed edges between individuals that
    /// never co-occur in a seed edge — may be missed whenever this is
    /// `true`. `false` only when the `TBox` is in-fragment AND the seed alone
    /// was returned (no extension candidates).
    #[must_use]
    pub fn incomplete(&self) -> bool {
        self.incomplete
    }
}

/// Entailed DATA property quads over named individuals, plus a completeness
/// flag.
#[derive(Debug, Clone)]
pub struct DataPropertyValues {
    quads: Vec<(String, String, String, String)>,
    incomplete: bool,
}

impl DataPropertyValues {
    /// `(subject_iri, property_iri, lexical, datatype_iri)` 4-tuples, sorted
    /// and deduplicated (the `lang` element of the underlying materialize
    /// closure is dropped — see the module doc).
    #[must_use]
    pub fn quads(&self) -> &[(String, String, String, String)] {
        &self.quads
    }

    /// Always `false`: `inferred_data_property_values` is a pure structural
    /// passthrough over `materialize_data_property_assertions`, which is
    /// complete for its own (structural) fragment. Kept as a field (not a
    /// constant) so the type stays parallel to `ObjectPropertyValues` and free
    /// to grow a real entailment extension later without an API break.
    #[must_use]
    pub fn incomplete(&self) -> bool {
        self.incomplete
    }
}

/// Named object properties declared in `internal` — i.e. genuine
/// `Axiom::DeclareObjectProperty` entries. Deliberately NOT
/// `vocabulary.roles()`: that namespace also holds data properties lowered to
/// roles (first-class data properties, `RUSTDL_DATA_PROPERTIES`), which must
/// not appear in the object-property candidate cross product.
fn named_object_properties(internal: &owl_dl_core::ontology::InternalOntology) -> Vec<RoleId> {
    let mut out: Vec<RoleId> = internal
        .axioms
        .iter()
        .filter_map(|ax| match ax {
            Axiom::DeclareObjectProperty(r) => Some(*r),
            _ => None,
        })
        .collect();
    out.sort();
    out.dedup();
    out
}

/// Canonical `(lo, hi)` ordering by index for a symmetric pair key.
fn canon_key(a: IndividualId, b: IndividualId) -> (u32, u32) {
    let (x, y) = (a.index(), b.index());
    if x <= y { (x, y) } else { (y, x) }
}

/// The bounded candidate-pair policy: distinct named-individual pairs that
/// co-occur in at least one seed edge (in either direction). This is
/// deliberately NOT the full `|individuals|²` cross product — it is bounded by
/// the number of DISTINCT pairs actually touched by the seed, which is at most
/// `2 × |seed edges|`. Both orderings of each unordered pair are candidates
/// (an entailed edge need not run in the same direction as the seed edge that
/// put the pair in the neighborhood — e.g. an inverse-role entailment).
fn candidate_extension_pairs(
    seed: &[(IndividualId, RoleId, IndividualId)],
) -> Vec<(IndividualId, IndividualId)> {
    let mut seen: HashSet<(u32, u32)> = HashSet::new();
    let mut pairs: Vec<(IndividualId, IndividualId)> = Vec::new();
    for &(a, _, b) in seed {
        if seen.insert(canon_key(a, b)) {
            pairs.push((a, b));
            if a != b {
                pairs.push((b, a));
            }
        }
    }
    pairs
}

/// Inferred OBJECT property values over named individuals: the sound
/// `materialize_object_property_assertions` seed, extended by a bounded
/// entailment probe over the seed's own individual-pair neighborhood (see the
/// module doc and [`candidate_extension_pairs`]). `pair_deadline` bounds each
/// extension probe; `None` = unbounded.
///
/// # Errors
/// [`ReasonError::Inconsistent`] if the ontology is inconsistent (everything
/// is vacuously entailed); [`ReasonError::Conversion`] on lowering failure.
pub fn inferred_object_property_values<A: ForIRI>(
    onto: &SetOntology<A>,
    pair_deadline: Option<Duration>,
) -> Result<ObjectPropertyValues, ReasonError> {
    // Sound lower-bound seed; already runs the inconsistent-KB guard and
    // returns `Err(Inconsistent)` on a clash.
    let seed = crate::materialize_object_property_assertions(onto)?;
    let mut seed_set: HashSet<(String, String, String)> = seed.iter().cloned().collect();
    let mut triples = seed.clone();

    let internal = convert_ontology(onto)?;
    let object_properties = named_object_properties(&internal);

    // Honest `incomplete` initialization (review Fix 2): the seed
    // (`materialize_object_property_assertions`, a Horn-only fixpoint over
    // NAMED individuals — see `abox_saturation`) plus the bounded
    // seed-neighborhood extension below are a sound UNDER-approximation
    // whenever the TBox carries constructs that Horn-only propagation does
    // not reason over (general disjunction, cardinality-driven case-splits,
    // ...) — e.g. `C ⊑ (∃R.{b} ⊔ ∃R.{c})`, which never seeds an edge and
    // whose candidate-pair neighborhood is therefore empty, so the extension
    // loop below never runs and never gets a chance to set `incomplete`
    // itself. Mirrors `disjointness.rs`'s
    // `!classification.completeness_guaranteed()` gate: `PureEl`/`Horn` ⟹
    // every entailed fact over named individuals is Horn-derivable, so the
    // seed is complete and starting `incomplete` at `false` is honest.
    // `analyze_fragment` (not the full `Classification`) is used here
    // because it is a pure TBox-shape check with no per-pair classify cost —
    // this function never needs a class hierarchy. Role CHARACTERISTIC
    // axioms (`Symmetric`/`Inverse`/`Transitive`/...) are handled completely
    // by the `ABox` saturator itself regardless of fragment (that closure is
    // exactly what the seed already reflects), and `analyze_fragment`'s
    // clausifier does not clausify ABox axioms or these role-characteristic
    // axioms at all, so they never push a fixture out of `PureEl`/`Horn` —
    // see `object_values_include_asserted_and_symmetric` /
    // `object_property_values_matches_hermit_oracle`, both of which stay
    // `PureEl`/`Horn` under this gate.
    let mut incomplete = !matches!(
        crate::classify::analyze_fragment(&internal),
        crate::classify::FragmentClassification::PureEl
            | crate::classify::FragmentClassification::Horn
    );

    // `from_internal` clones `internal.vocabulary` before consuming
    // `internal`, so `prepared.vocabulary` resolves the same IRI ↔ id mapping
    // used to build the seed triples above.
    let prepared = PreparedOntology::from_internal(internal)?;
    let vocab = &prepared.vocabulary;

    // Resolve the seed's string triples back to ids so the candidate-pair
    // neighborhood can be computed over ids (needed for `consistent_with_extra`).
    let mut seed_ids: Vec<(IndividualId, RoleId, IndividualId)> = Vec::new();
    for (s, p, o) in &seed {
        if let (Some(a), Some(r), Some(b)) = (
            vocab.individual_id(s),
            vocab.role_id(p),
            vocab.individual_id(o),
        ) {
            seed_ids.push((a, r, b));
        }
    }

    let candidate_pairs = candidate_extension_pairs(&seed_ids);
    for (a, b) in candidate_pairs {
        let a_iri = vocab.individual_iri(a).to_string();
        let b_iri = vocab.individual_iri(b).to_string();
        for &r in &object_properties {
            let r_iri = vocab.role_iri(r).to_string();
            let candidate = (a_iri.clone(), r_iri, b_iri.clone());
            if seed_set.contains(&candidate) {
                continue;
            }
            incomplete = true;
            let deadline = pair_deadline.map(|d| Instant::now() + d);
            if prepared.consistent_with_extra(&[], &[(a, r, b)], deadline)? == Some(false) {
                // KB ∪ {¬R(a,b)} is inconsistent ⇒ R(a,b) is entailed.
                seed_set.insert(candidate.clone());
                triples.push(candidate);
            }
        }
    }

    triples.sort();
    triples.dedup();
    Ok(ObjectPropertyValues {
        triples,
        incomplete,
    })
}

/// Inferred DATA property values over named individuals: a pure structural
/// passthrough over `materialize_data_property_assertions`, dropping the
/// `lang` element to a 4-tuple. See the module doc for the v1 scope boundary
/// (no negative-data-assertion entailment extension).
///
/// # Errors
/// [`ReasonError::Inconsistent`] if the ontology is inconsistent;
/// [`ReasonError::Conversion`] on lowering failure.
pub fn inferred_data_property_values<A: ForIRI>(
    onto: &SetOntology<A>,
) -> Result<DataPropertyValues, ReasonError> {
    let quints = crate::materialize_data_property_assertions(onto)?;
    let mut quads: Vec<(String, String, String, String)> = quints
        .into_iter()
        .map(|(s, p, lex, dt, _lang)| (s, p, lex, dt))
        .collect();
    quads.sort();
    quads.dedup();
    Ok(DataPropertyValues {
        quads,
        incomplete: false,
    })
}
