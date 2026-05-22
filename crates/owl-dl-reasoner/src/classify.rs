//! Class hierarchy computation — naive O(n²) baseline.
//!
//! For each ordered pair `(C, D)` of named classes in the input,
//! decide `C ⊑ D` via the standard satisfiability reduction
//! ([`crate::is_subclass_of_internal`]). The full pairwise matrix is
//! retained; convenience accessors derive equivalence classes, the
//! Hasse-direct super-class relation, and the set of classes
//! equivalent to `⊥` (unsatisfiable).
//!
//! This is *correct* but not fast — every pair triggers a fresh
//! pipeline pass (axiom expansion + NNF + absorption + tableau).
//! Phase 6's consequence-based saturation engine is the planned
//! acceleration.

use std::collections::{HashMap, HashSet};
use std::time::Instant;

use horned_owl::model::ForIRI;
use horned_owl::ontology::set::SetOntology;
use rayon::prelude::*;

use owl_dl_core::convert::convert_ontology;
use owl_dl_core::{Axiom, ConceptExpr, ConceptId, ConceptPool, InternalOntology, SubRolePath};
use owl_dl_saturation::saturate;

use crate::{PreparedOntology, ReasonError};

/// `(i, j, entailed, used_saturation, timed_out)` — one entry per
/// pairwise subsumption check returned from the parallel work loop.
type PairResult = (usize, usize, bool, bool, bool);

/// Result of [`classify`]. Holds the complete pairwise subsumption
/// matrix over every declared named class plus the IRIs themselves,
/// keyed by stable insertion order.
#[derive(Debug, Clone)]
pub struct Classification {
    classes: Vec<String>,
    index: HashMap<String, usize>,
    /// `entailed[i][j]` is true iff `classes[i] ⊑ classes[j]` in the
    /// input ontology (including reflexive entries `i == j`). Stored
    /// as a row-major bit-vector via `Vec<bool>`.
    entailed: Vec<Vec<bool>>,
    unsatisfiable_idxs: HashSet<usize>,
    stats: ClassificationStats,
}

/// Per-call instrumentation: who decided what during the pairwise
/// classification loop. Useful for understanding when the EL
/// saturation oracle is pulling its weight versus when the tableau
/// is doing the work.
#[derive(Debug, Clone, Copy, Default)]
pub struct ClassificationStats {
    /// Pairwise subsumption queries answered `yes` by saturation's
    /// EL closure (no tableau call issued).
    pub saturation_subsumption_hits: usize,
    /// Pairwise subsumption queries that the saturation closure did
    /// not witness, dispatched to the tableau.
    pub tableau_subsumption_calls: usize,
    /// Classes flagged as `⊑ ⊥` by saturation directly (no per-class
    /// tableau probe issued).
    pub saturation_unsat_hits: usize,
    /// Classes that needed a per-class tableau satisfiability probe
    /// (saturation had no opinion).
    pub tableau_unsat_calls: usize,
    /// True iff the entire ontology fits inside the EL fragment
    /// our saturation engine is complete for — in that case the
    /// tableau is never invoked and saturation's `no` answer is
    /// itself the verdict (`no` pairs aren't counted in
    /// `saturation_subsumption_hits`, only the `yes` pairs are).
    pub pure_el_mode: bool,
    /// When the classifier was configured with a per-pair timeout,
    /// the number of pairs that hit it before the tableau returned
    /// a verdict. Those pairs default to `not subsumed` in the
    /// entailment matrix — sound (never reports a false positive),
    /// but may under-report subsumption.
    pub timed_out_pairs: usize,
}

impl Classification {
    /// Every declared class IRI in insertion order.
    #[must_use]
    pub fn classes(&self) -> &[String] {
        &self.classes
    }

    /// True iff `sub ⊑ sup` is entailed by the ontology.
    /// Returns `false` if either IRI is not a declared class
    /// (callers wanting a hard error should use
    /// [`crate::is_subclass_of`] directly).
    #[must_use]
    pub fn is_subclass(&self, sub: &str, sup: &str) -> bool {
        let (Some(&i), Some(&j)) = (self.index.get(sub), self.index.get(sup)) else {
            return false;
        };
        self.entailed[i][j]
    }

    /// All classes equivalent to `c` (including `c` itself). Empty if
    /// `c` is not declared in the ontology.
    #[must_use]
    pub fn equivalent_classes(&self, c: &str) -> Vec<&str> {
        let Some(&i) = self.index.get(c) else {
            return Vec::new();
        };
        (0..self.classes.len())
            .filter(|&j| self.entailed[i][j] && self.entailed[j][i])
            .map(|j| self.classes[j].as_str())
            .collect()
    }

    /// The Hasse-direct super-classes of `c`: every `D` with
    /// `c ⊑ D`, `D ≢ c`, and no intermediate `E ≠ c, D` such that
    /// `c ⊑ E ⊑ D`. Empty if `c` is not declared.
    #[must_use]
    pub fn direct_subsumers(&self, c: &str) -> Vec<&str> {
        let Some(&i) = self.index.get(c) else {
            return Vec::new();
        };
        let n = self.classes.len();
        // First: every strict super (i ⊑ j, not j ⊑ i).
        let strict_supers: Vec<usize> = (0..n)
            .filter(|&j| j != i && self.entailed[i][j] && !self.entailed[j][i])
            .collect();
        // Then: prune any `j` for which there is a `k` strictly
        // between i and j (i ⊑ k ⊑ j, neither equivalent).
        strict_supers
            .iter()
            .copied()
            .filter(|&j| {
                !strict_supers
                    .iter()
                    .any(|&k| k != j && self.entailed[k][j] && !self.entailed[j][k])
            })
            .map(|j| self.classes[j].as_str())
            .collect()
    }

    /// Per-call instrumentation for this classification: how many
    /// subsumption queries each engine handled, and how many
    /// unsatisfiable classes each engine flagged.
    #[must_use]
    pub fn stats(&self) -> ClassificationStats {
        self.stats
    }

    /// All declared classes that are equivalent to `⊥` — i.e. classes
    /// the ontology proves to be empty.
    #[must_use]
    pub fn unsatisfiable_classes(&self) -> Vec<&str> {
        let mut out: Vec<&str> = self
            .unsatisfiable_idxs
            .iter()
            .map(|&i| self.classes[i].as_str())
            .collect();
        out.sort_unstable();
        out
    }
}

/// Compute the full subsumption hierarchy of `ontology` over every
/// declared named class. Returns a [`Classification`] from which
/// callers can query subsumption, equivalence, direct super-classes,
/// and the unsatisfiable-class set.
///
/// # Errors
///
/// See [`ReasonError`]. Any single subsumption check that errors
/// (e.g. an unsupported role chain) aborts classification with that
/// error — partial results are not surfaced.
pub fn classify<A: ForIRI>(ontology: &SetOntology<A>) -> Result<Classification, ReasonError> {
    let internal = convert_ontology(ontology)?;
    classify_internal(&internal)
}

/// Like [`classify`] but each pairwise tableau query is bounded by
/// `per_pair_timeout`. Pairs that exceed the timeout default to
/// `not subsumed` in the entailment matrix (sound under-approximation)
/// and bump [`ClassificationStats::timed_out_pairs`].
///
/// # Errors
///
/// See [`ReasonError`].
pub fn classify_with_timeout<A: ForIRI>(
    ontology: &SetOntology<A>,
    per_pair_timeout: std::time::Duration,
) -> Result<Classification, ReasonError> {
    let internal = convert_ontology(ontology)?;
    classify_internal_with_timeout(&internal, Some(per_pair_timeout))
}

/// Internal entry point. Useful for tests that hand-build an
/// [`InternalOntology`].
///
/// # Errors
///
/// See [`ReasonError`].
pub fn classify_internal(internal: &InternalOntology) -> Result<Classification, ReasonError> {
    classify_internal_with_timeout(internal, None)
}

#[allow(clippy::too_many_lines)]
pub(crate) fn classify_internal_with_timeout(
    internal: &InternalOntology,
    per_pair_timeout: Option<std::time::Duration>,
) -> Result<Classification, ReasonError> {
    // Snapshot the class IRIs before we clone the ontology into each
    // subsumption call. Order is the vocabulary's interning order.
    let classes: Vec<String> = (0..internal.vocabulary.num_classes())
        .map(|i| {
            internal
                .vocabulary
                .class_iri(owl_dl_core::ClassId::new(
                    u32::try_from(i).expect("class count fits in u32"),
                ))
                .to_owned()
        })
        .collect();
    let n = classes.len();
    let index: HashMap<String, usize> = classes
        .iter()
        .enumerate()
        .map(|(i, iri)| (iri.clone(), i))
        .collect();

    // Run the EL saturation engine once up-front. Its closure is
    // *sound* (every entry is a genuine entailment, and every
    // `is_unsatisfiable` flag is a real ⊥) but only complete for the
    // EL fragment of the input — so we use it as a fast positive
    // oracle and fall back to the tableau when the closure has
    // nothing to say.
    let closure = saturate(internal);

    // If the entire ontology fits inside the EL fragment our
    // saturation engine recognises, the closure is *also complete*
    // — saturation's `no` answer is itself the verdict, and we
    // never need the tableau. This is the common case for partonomy
    // ontologies like Galen-EL or the SNOMED core fragment.
    if is_pure_el(internal) {
        return Ok(classify_pure_el(internal, &classes, &index, &closure));
    }

    // Prepare the tableau-side pipeline once. Every subsequent
    // tableau query reuses the absorbed TBox, role-side metadata,
    // ABox seed, and pool — only the test concept varies.
    let prepared = PreparedOntology::from_internal(internal.clone())?;

    // First pass: which classes are individually unsatisfiable? An
    // unsat class `C` is `⊑ ⊥` and therefore `⊑ D` for every `D` —
    // record that directly. Saturation's bot-detection flags many of
    // these without ever invoking the tableau; the rest fall back to
    // a per-class satisfiability probe. Probes are independent so
    // they run in parallel via rayon.
    let mut stats = ClassificationStats::default();
    let unsat_probe_results: Result<Vec<(usize, bool, bool)>, ReasonError> = (0..n)
        .into_par_iter()
        .map(|i| {
            let class_id =
                owl_dl_core::ClassId::new(u32::try_from(i).expect("class index fits in u32"));
            if closure.is_unsatisfiable(class_id) {
                // (idx, is_sat, used_saturation)
                Ok((i, false, true))
            } else if let Some(timeout) = per_pair_timeout {
                let deadline = Instant::now() + timeout;
                // A timed-out unsat probe defaults to "satisfiable" —
                // sound: if the class actually were unsat the timeout
                // would have flagged it via saturation already, and
                // assuming sat here can never cause us to claim a
                // false subsumption later.
                let sat = prepared
                    .decide_with_deadline(deadline, move |pool| pool.atomic(class_id))?
                    .unwrap_or(true);
                Ok((i, sat, false))
            } else {
                let sat = prepared.decide(move |pool| pool.atomic(class_id))?;
                Ok((i, sat, false))
            }
        })
        .collect();
    let unsat_probe_results = unsat_probe_results?;
    let mut unsatisfiable_idxs: HashSet<usize> = HashSet::new();
    let mut satisfiable: Vec<bool> = vec![false; n];
    for (i, is_sat, used_saturation) in unsat_probe_results {
        if used_saturation {
            stats.saturation_unsat_hits += 1;
        } else {
            stats.tableau_unsat_calls += 1;
        }
        if is_sat {
            satisfiable[i] = true;
        } else {
            unsatisfiable_idxs.insert(i);
        }
    }

    // Second pass: pairwise subsumption. Build the worklist of
    // (i, j) pairs that need a real query (saturation-or-tableau);
    // run them in parallel; reduce into the entailment matrix and
    // stats counters. Skip rows where `i` is unsatisfiable (it
    // subsumes everything trivially — fill the row).
    let mut entailed: Vec<Vec<bool>> = vec![vec![false; n]; n];
    let mut work: Vec<(usize, usize)> = Vec::new();
    #[allow(clippy::needless_range_loop)]
    for i in 0..n {
        entailed[i][i] = true;
        if unsatisfiable_idxs.contains(&i) {
            entailed[i].iter_mut().take(n).for_each(|v| *v = true);
            continue;
        }
        for j in 0..n {
            if i == j || unsatisfiable_idxs.contains(&j) {
                continue;
            }
            work.push((i, j));
        }
    }
    let pair_results: Result<Vec<PairResult>, ReasonError> = work
        .par_iter()
        .map(|&(i, j)| {
            let sub_class =
                owl_dl_core::ClassId::new(u32::try_from(i).expect("class index fits in u32"));
            let super_class =
                owl_dl_core::ClassId::new(u32::try_from(j).expect("class index fits in u32"));
            if closure.contains(sub_class, super_class) {
                // (i, j, entailed, used_saturation, timed_out)
                return Ok((i, j, true, true, false));
            }
            let build = move |pool: &mut ConceptPool| {
                let sub_concept = pool.atomic(sub_class);
                let super_concept = pool.atomic(super_class);
                let not_super = pool.not(super_concept);
                pool.and(vec![sub_concept, not_super])
            };
            match per_pair_timeout {
                None => {
                    let sat = prepared.decide(build)?;
                    Ok((i, j, !sat, false, false))
                }
                Some(timeout) => {
                    // Cooperative deadline: the tableau's search loop
                    // checks `Instant::now()` against this deadline on
                    // every recursion and bails out if exceeded. No
                    // extra threads, no cancellation race — the rayon
                    // worker stays bound to this single decide call.
                    let deadline = Instant::now() + timeout;
                    match prepared.decide_with_deadline(deadline, build)? {
                        Some(sat) => Ok((i, j, !sat, false, false)),
                        None => Ok((i, j, false, false, true)),
                    }
                }
            }
        })
        .collect();
    for (i, j, is_entailed, used_saturation, timed_out) in pair_results? {
        if timed_out {
            stats.timed_out_pairs += 1;
            // Sound under-approximation: default to "not subsumed".
            // Do not credit either engine — neither produced a verdict.
            continue;
        }
        if used_saturation {
            stats.saturation_subsumption_hits += 1;
        } else {
            stats.tableau_subsumption_calls += 1;
        }
        entailed[i][j] = is_entailed;
    }
    let _ = satisfiable; // currently informational only
    Ok(Classification {
        classes,
        index,
        entailed,
        unsatisfiable_idxs,
        stats,
    })
}

/// Fast-path classifier for ontologies that lie entirely inside our
/// EL saturation fragment. The closure is then *complete* — both
/// subsumption and unsatisfiability decisions reduce to closure
/// lookups, with no tableau calls. Sets `stats.pure_el_mode = true`.
fn classify_pure_el(
    internal: &InternalOntology,
    classes: &[String],
    index: &HashMap<String, usize>,
    closure: &owl_dl_saturation::Subsumers,
) -> Classification {
    let n = classes.len();
    let mut stats = ClassificationStats {
        pure_el_mode: true,
        ..ClassificationStats::default()
    };
    let mut unsatisfiable_idxs: HashSet<usize> = HashSet::new();
    let mut entailed: Vec<Vec<bool>> = vec![vec![false; n]; n];
    for (i, row) in entailed.iter_mut().enumerate().take(n) {
        row[i] = true;
        let class_id =
            owl_dl_core::ClassId::new(u32::try_from(i).expect("class index fits in u32"));
        if closure.is_unsatisfiable(class_id) {
            unsatisfiable_idxs.insert(i);
            stats.saturation_unsat_hits += 1;
            for v in row.iter_mut() {
                *v = true;
            }
        }
    }
    #[allow(clippy::needless_range_loop)]
    for i in 0..n {
        if unsatisfiable_idxs.contains(&i) {
            continue;
        }
        let sub_class =
            owl_dl_core::ClassId::new(u32::try_from(i).expect("class index fits in u32"));
        #[allow(clippy::needless_range_loop)]
        for j in 0..n {
            if i == j || unsatisfiable_idxs.contains(&j) {
                continue;
            }
            let super_class =
                owl_dl_core::ClassId::new(u32::try_from(j).expect("class index fits in u32"));
            if closure.contains(sub_class, super_class) {
                entailed[i][j] = true;
                stats.saturation_subsumption_hits += 1;
            }
        }
    }
    let _ = internal; // closure was built from this; nothing more to read
    Classification {
        classes: classes.to_vec(),
        index: index.clone(),
        entailed,
        unsatisfiable_idxs,
        stats,
    }
}

/// True iff every axiom in `internal` lies inside the EL fragment
/// the saturation engine is complete for. A conservative check: any
/// construct outside the supported shapes (disjunction, complement,
/// cardinality, nominals, inverse roles, role characteristics that
/// expand to cardinality, `ABox` assertions, ...) immediately returns
/// `false`.
pub(crate) fn is_pure_el(internal: &InternalOntology) -> bool {
    internal
        .axioms
        .iter()
        .all(|ax| is_el_axiom(ax, &internal.concepts))
}

fn is_el_axiom(ax: &Axiom, pool: &ConceptPool) -> bool {
    match ax {
        Axiom::SubClassOf { sub, sup } => is_el_concept(*sub, pool) && is_el_concept(*sup, pool),
        Axiom::EquivalentClasses(members) => members.iter().all(|c| is_el_concept(*c, pool)),
        Axiom::DisjointClasses(members) => members.iter().all(|c| is_el_concept(*c, pool)),
        Axiom::SubObjectPropertyOf { sub, sup } => {
            if sup.is_inverse() {
                return false;
            }
            match sub {
                SubRolePath::Role(r) => !r.is_inverse(),
                SubRolePath::Chain(parts) => {
                    parts.len() == 2 && parts.iter().all(|r| !r.is_inverse())
                }
            }
        }
        Axiom::EquivalentObjectProperties(roles) => roles.iter().all(|r| !r.is_inverse()),
        Axiom::TransitiveRole(role) => !role.is_inverse(),
        Axiom::ObjectPropertyDomain { role, domain } => {
            !role.is_inverse() && is_el_concept(*domain, pool)
        }
        Axiom::ObjectPropertyRange { role, range } => {
            !role.is_inverse() && is_el_concept(*range, pool)
        }
        Axiom::DeclareClass(_)
        | Axiom::DeclareObjectProperty(_)
        | Axiom::DeclareNamedIndividual(_) => true,
        // Everything else (ABox assertions, role characteristics that
        // expand to cardinality, disjoint object properties, ...) is
        // outside the saturation fragment.
        _ => false,
    }
}

fn is_el_concept(c: ConceptId, pool: &ConceptPool) -> bool {
    match pool.get(c) {
        ConceptExpr::Top | ConceptExpr::Atomic(_) => true,
        ConceptExpr::And(ops) => ops.iter().all(|op| is_el_concept(*op, pool)),
        ConceptExpr::Some(role, body) => !role.is_inverse() && is_el_concept(*body, pool),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use horned_owl::io::ParserConfiguration;
    use horned_owl::io::ofn::reader::read;
    use horned_owl::model::RcStr;
    use horned_owl::ontology::set::SetOntology;
    use std::io::Cursor;

    fn parse(src: &str) -> SetOntology<RcStr> {
        let mut reader = Cursor::new(src);
        let (ontology, _prefixes) =
            read(&mut reader, ParserConfiguration::default()).expect("fixture parses");
        ontology
    }

    const HEADER: &str = "\
Prefix(:=<http://rustdl.test/>)\n\
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)\n";

    #[test]
    fn classify_picks_up_explicit_chain() {
        // A ⊑ B ⊑ C — classification should yield both direct edges.
        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(Class(:B))\n\
    Declaration(Class(:C))\n\
    SubClassOf(:A :B)\n\
    SubClassOf(:B :C)\n\
)\n"
        ));
        let h = classify(&onto).expect("classification");
        let iri = |s: &str| format!("http://rustdl.test/{s}");
        assert!(h.is_subclass(&iri("A"), &iri("B")));
        assert!(h.is_subclass(&iri("B"), &iri("C")));
        assert!(h.is_subclass(&iri("A"), &iri("C")));
        assert!(!h.is_subclass(&iri("C"), &iri("A")));
        let direct = h.direct_subsumers(&iri("A"));
        assert_eq!(direct, vec![iri("B")]);
    }

    #[test]
    fn classify_groups_equivalents() {
        // EquivalentClasses(A, B) — they should appear as each
        // other's equivalents.
        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(Class(:B))\n\
    EquivalentClasses(:A :B)\n\
)\n"
        ));
        let h = classify(&onto).expect("classification");
        let iri_a = "http://rustdl.test/A".to_string();
        let iri_b = "http://rustdl.test/B".to_string();
        let equiv_a: Vec<String> = h
            .equivalent_classes(&iri_a)
            .into_iter()
            .map(str::to_owned)
            .collect();
        assert!(equiv_a.contains(&iri_a));
        assert!(equiv_a.contains(&iri_b));
    }

    #[test]
    fn classify_flags_unsatisfiable() {
        // A ⊑ B ⊓ ¬B — A is empty, equivalent to ⊥.
        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(Class(:B))\n\
    SubClassOf(:A ObjectIntersectionOf(:B ObjectComplementOf(:B)))\n\
)\n"
        ));
        let h = classify(&onto).expect("classification");
        assert!(h.unsatisfiable_classes().contains(&"http://rustdl.test/A"));
    }

    #[test]
    fn classify_stats_show_saturation_carries_pure_el() {
        // Pure EL: A ⊑ B ⊑ C ⊑ D. Saturation should handle every
        // (non-reflexive, non-self) pairwise subsumption query
        // without dispatching to the tableau.
        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(Class(:B))\n\
    Declaration(Class(:C))\n\
    Declaration(Class(:D))\n\
    SubClassOf(:A :B)\n\
    SubClassOf(:B :C)\n\
    SubClassOf(:C :D)\n\
)\n"
        ));
        let h = classify(&onto).expect("classification");
        let stats = h.stats();
        // Pure EL ⇒ tableau is never invoked, saturation alone is
        // both sound and complete here.
        assert!(stats.pure_el_mode);
        assert_eq!(stats.tableau_subsumption_calls, 0);
        assert_eq!(stats.tableau_unsat_calls, 0);
        // Entailed pairs are A⊑B, A⊑C, A⊑D, B⊑C, B⊑D, C⊑D = 6.
        assert_eq!(stats.saturation_subsumption_hits, 6);
    }

    #[test]
    fn classify_with_timeout_matches_untimed_for_simple_input() {
        // A → B → C (pure EL) — even with a tiny timeout, all pairs
        // get answered by saturation (the closure path bypasses the
        // tableau entirely) so the timed and untimed runs agree.
        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(Class(:B))\n\
    Declaration(Class(:C))\n\
    SubClassOf(:A :B)\n\
    SubClassOf(:B :C)\n\
)\n"
        ));
        let baseline = classify(&onto).expect("baseline");
        let timed = super::classify_with_timeout(&onto, std::time::Duration::from_millis(50))
            .expect("timed classification");
        assert_eq!(baseline.stats().timed_out_pairs, 0);
        assert_eq!(timed.stats().timed_out_pairs, 0);
        let iri = |s: &str| format!("http://rustdl.test/{s}");
        assert!(timed.is_subclass(&iri("A"), &iri("C")));
        assert_eq!(
            baseline.unsatisfiable_classes(),
            timed.unsatisfiable_classes()
        );
    }

    #[test]
    fn classify_drops_to_tableau_when_axioms_leave_el() {
        // The DisjointObjectProperties axiom is outside our EL
        // saturation fragment — classify should NOT take the
        // pure-EL fast path and should issue at least one tableau
        // call (per-class unsat probes count).
        let onto = parse(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(Class(:B))\n\
    Declaration(ObjectProperty(:r))\n\
    Declaration(ObjectProperty(:s))\n\
    DisjointObjectProperties(:r :s)\n\
    SubClassOf(:A :B)\n\
)\n"
        ));
        let h = classify(&onto).expect("classification");
        let stats = h.stats();
        assert!(!stats.pure_el_mode);
        assert!(
            stats.tableau_subsumption_calls + stats.tableau_unsat_calls > 0,
            "expected the tableau to be invoked for the non-EL fragment"
        );
    }
}
