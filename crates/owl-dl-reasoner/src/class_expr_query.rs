//! Complex (anonymous) class-expression queries (issue #48). A parsed
//! `ClassExpression` is answered by minting a fresh probe class `Q`, adding
//! `EquivalentClasses(Q, CE)`, and delegating to the named-class queries — the
//! same reduction `justify::entails` uses for its `DisjointClasses` arm. Sound by
//! construction (a definitional extension over a fresh name adds no entailment
//! about the original signature). Read-only on the input.

use std::collections::HashSet;

use crate::{QueryStats, ReasonError};
use horned_owl::model::{
    Build, ClassExpression, Component, EquivalentClasses, ForIRI, MutableOntology,
};
use horned_owl::ontology::set::SetOntology;
use owl_dl_core::dropped::DroppedAxioms;

const PROBE_IRI: &str = "urn:rustdl-ce-probe:q";
const PROBE_IRI_2: &str = "urn:rustdl-ce-probe:q2";

/// Verdict for a complex-class-expression satisfiability/subsumption query.
// #540: not `Copy` any more; it carries the probed conversion's `dropped`.
#[derive(Debug, Clone)]
#[allow(clippy::struct_excessive_bools)]
pub struct CeVerdict {
    holds: bool,
    incomplete: bool,
    completeness_guaranteed: bool,
    relies_on_trusted_consistency: bool,
    consistency_undetermined: bool,
    no_verdict: bool,
    dropped: DroppedAxioms,
}

impl CeVerdict {
    /// #462: the deadline-free probe ended in `NoVerdict`, an internal limit (likely a
    /// reasoner bug, #449). [`Self::holds`] is then the sound direction ("satisfiable" /
    /// "not entailed") and [`Self::incomplete`] is set too.
    fn from_no_verdict(holds: bool, dropped: DroppedAxioms) -> Self {
        Self {
            holds,
            incomplete: true,
            completeness_guaranteed: false,
            relies_on_trusted_consistency: false,
            consistency_undetermined: false,
            no_verdict: true,
            dropped,
        }
    }

    /// Whether the query holds.
    #[must_use]
    pub fn holds(&self) -> bool {
        self.holds
    }

    /// The probe gave up (node cap) and [`Self::holds`] is the sound direction of
    /// no verdict: "satisfiable" / "not entailed" (#363 A2). Only a give-up: an
    /// answer off the pure-EL fast path is not `incomplete`; see
    /// [`Self::completeness_guaranteed`]. (Before package 2's prerequisite this
    /// was also set whenever the ontology was not pure EL.)
    #[must_use]
    pub fn incomplete(&self) -> bool {
        self.incomplete
    }

    /// The verdict is guaranteed complete: answered on the pure-EL fast path,
    /// nothing gave up, and conversion dropped no axiom (#540). `false` does not mean the verdict is wrong, only
    /// that a "satisfiable" / "not entailed" is not proven complete (the same
    /// meaning as `classify --json`'s `completeness_guaranteed`).
    #[must_use]
    pub fn completeness_guaranteed(&self) -> bool {
        self.completeness_guaranteed
    }

    /// #401: the verdict was reached with the `ABox` seed dropped, on the strength
    /// of a trusted wedge `Sat` for consistency. See
    /// [`QueryStats::relies_on_trusted_consistency`].
    #[must_use]
    pub fn relies_on_trusted_consistency(&self) -> bool {
        self.relies_on_trusted_consistency
    }

    /// #369: the answer was positive ("unsatisfiable" / "subsumed") and the KB's
    /// consistency stayed undetermined (the consistency wedge stalled or ran past
    /// `RUSTDL_QUERY_LAZY_CONSISTENCY_MS`), so it may be the vacuous answer of an
    /// inconsistent KB. Not a give-up, so separate from [`Self::incomplete`]. See
    /// [`crate::InstanceFlags::consistency_undetermined`].
    #[must_use]
    pub fn consistency_undetermined(&self) -> bool {
        self.consistency_undetermined
    }

    /// #462: the deadline-free probe ended in `NoVerdict` (an internal limit, likely a
    /// reasoner bug, #449) rather than a verdict. [`Self::incomplete`] is set too; this
    /// is its own signal so a caller can tell it from a node-cap give-up.
    #[must_use]
    pub fn no_verdict(&self) -> bool {
        self.no_verdict
    }

    /// #540: the axioms the probed ontology's conversion dropped: the input's
    /// and the query expression's own (the probe axiom `Q ≡ ce` is converted with
    /// the input). Non-empty voids [`Self::completeness_guaranteed`]; a caller
    /// should name these, not the input's drops alone.
    #[must_use]
    pub fn dropped(&self) -> &DroppedAxioms {
        &self.dropped
    }
}

/// Result of a complex-class-expression instance query.
// `struct_excessive_bools`: each bool is an independent signal of the public result
// (#493 added `no_verdict`); they are not states of one machine.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone)]
pub struct CeInstances {
    individuals: Vec<String>,
    incomplete: bool,
    completeness_guaranteed: bool,
    consistency_undetermined: bool,
    no_verdict: bool,
    depth_capped: bool,
    dropped: DroppedAxioms,
}

impl CeInstances {
    /// The individuals provably in the queried class expression.
    #[must_use]
    pub fn individuals(&self) -> &[String] {
        &self.individuals
    }

    /// At least one instance probe was cut (deadline, node cap or depth
    /// limit), so members may be missing; the members returned are sound
    /// (#363 A2). Only a give-up; see [`Self::completeness_guaranteed`].
    #[must_use]
    pub fn incomplete(&self) -> bool {
        self.incomplete
    }

    /// The member list is guaranteed complete: the instance query ran on the
    /// realize saturation fast path (complete on its fragment), so nothing could
    /// be cut, and conversion dropped no axiom (#540). Off that fragment the
    /// tableau path's pseudo-model prune is not proven complete, so this is
    /// `false` even when no probe was cut.
    #[must_use]
    pub fn completeness_guaranteed(&self) -> bool {
        self.completeness_guaranteed
    }

    /// #369: some individual was found, and the KB's consistency stayed
    /// undetermined. See [`crate::InstanceFlags::consistency_undetermined`].
    #[must_use]
    pub fn consistency_undetermined(&self) -> bool {
        self.consistency_undetermined
    }

    /// #493: an instance probe ended in `NoVerdict` as an internal limit, likely a
    /// reasoner bug (#449): a probe with no deadline, or one whose saturation stalled
    /// (#475). [`Self::incomplete`] is set too. See
    /// [`crate::InstanceFlags::no_verdict`].
    #[must_use]
    pub fn no_verdict(&self) -> bool {
        self.no_verdict
    }

    /// #510: a cut instance probe hit the tableau's decision-depth limit rather than a
    /// deadline or the node cap, so a larger budget does not recover it. Report-only:
    /// implies [`Self::incomplete`]. See [`crate::InstanceFlags::depth_capped`].
    #[must_use]
    pub fn depth_capped(&self) -> bool {
        self.depth_capped
    }

    /// #540: as [`CeVerdict::dropped`].
    #[must_use]
    pub fn dropped(&self) -> &DroppedAxioms {
        &self.dropped
    }
}

/// `(incomplete, completeness_guaranteed)` of a satisfiability/subsumption
/// probe: `incomplete` is a give-up only (#395); the guarantee is
/// [`QueryStats::completeness_guaranteed`] (#540: over a conversion that dropped
/// nothing, counting the probe axioms; #552: with the KB's consistency decided and
/// not trusted), with the lazy check's `undetermined` folded in first.
fn verdict_flags(mut stats: QueryStats, lazy_undetermined: bool) -> (bool, bool) {
    stats.consistency_undetermined |= lazy_undetermined;
    (stats.incomplete, stats.completeness_guaranteed())
}

/// The completeness guarantee of an instance query (#540, #552): the list came off
/// the realize saturation fast path (complete on its fragment), no probe was cut,
/// conversion dropped nothing, the KB's consistency was not left undetermined (on an
/// inconsistent KB every individual is an instance, so a short list misses members),
/// and the eager inconsistency check ran (`RUSTDL_QUERY_INCONSISTENCY_GUARD`,
/// default on): with it off, the fast path answers without checking consistency at
/// all, and an inconsistent KB's list reads complete.
fn instances_guarantee(
    fast_path: bool,
    flags: crate::realize::InstanceFlags,
    dropped_nothing: bool,
    guard_ran: bool,
) -> bool {
    fast_path
        && !flags.incomplete
        && dropped_nothing
        && !flags.consistency_undetermined
        && guard_ran
}

/// Add every class IRI referenced (nested) inside `ce` to `out`. Mirrors
/// `justify::collect_ce_entities`'s recursion shape but only ever inserts on
/// the `ClassExpression::Class` arm — properties/individuals/data-property
/// fillers are irrelevant to "does this IRI denote a class".
fn collect_ce_classes<A: ForIRI>(ce: &ClassExpression<A>, out: &mut HashSet<String>) {
    use ClassExpression as CE;
    match ce {
        CE::Class(c) => {
            out.insert(c.0.as_ref().to_string());
        }
        CE::ObjectComplementOf(c) => collect_ce_classes(c, out),
        CE::ObjectIntersectionOf(cs) | CE::ObjectUnionOf(cs) => {
            for c in cs {
                collect_ce_classes(c, out);
            }
        }
        CE::ObjectSomeValuesFrom { bce, .. }
        | CE::ObjectAllValuesFrom { bce, .. }
        | CE::ObjectMinCardinality { bce, .. }
        | CE::ObjectMaxCardinality { bce, .. }
        | CE::ObjectExactCardinality { bce, .. } => collect_ce_classes(bce, out),
        // ObjectHasValue/ObjectHasSelf/ObjectOneOf (individuals, not classes) and
        // the Data* variants (datatype fillers) contribute no class IRI.
        _ => {}
    }
}

/// The full class signature of `onto`: every IRI that denotes a class
/// anywhere — `Declaration(Class(...))`, a `DisjointUnion` definiendum, or a
/// `ClassExpression::Class` occurring (recursively) in any axiom. Broader
/// than scanning declarations alone: an IRI can be *used* as a class in e.g.
/// `SubClassOf(<iri> Real)` with no accompanying `Declaration(Class(...))`,
/// and the probe-freshness invariant must catch that too (horned-owl 1.4 /
/// this pinned fork exposes no ready-made "ontology class signature" API —
/// no `signature()`/`Signatured` trait — so this walks `Component` by hand).
fn class_signature<A: ForIRI>(onto: &SetOntology<A>) -> HashSet<String> {
    let mut out = HashSet::new();
    for ac in onto {
        match &ac.component {
            Component::DeclareClass(dc) => {
                out.insert(dc.0.0.as_ref().to_string());
            }
            Component::SubClassOf(a) => {
                collect_ce_classes(&a.sub, &mut out);
                collect_ce_classes(&a.sup, &mut out);
            }
            Component::EquivalentClasses(a) => {
                for x in &a.0 {
                    collect_ce_classes(x, &mut out);
                }
            }
            Component::DisjointClasses(a) => {
                for x in &a.0 {
                    collect_ce_classes(x, &mut out);
                }
            }
            Component::DisjointUnion(a) => {
                out.insert(a.0.0.as_ref().to_string());
                for x in &a.1 {
                    collect_ce_classes(x, &mut out);
                }
            }
            Component::ObjectPropertyDomain(a) => collect_ce_classes(&a.ce, &mut out),
            Component::ObjectPropertyRange(a) => collect_ce_classes(&a.ce, &mut out),
            Component::DataPropertyDomain(a) => collect_ce_classes(&a.ce, &mut out),
            Component::HasKey(a) => collect_ce_classes(&a.ce, &mut out),
            Component::ClassAssertion(a) => collect_ce_classes(&a.ce, &mut out),
            _ => {}
        }
    }
    out
}

/// Error if `iri` already occurs as a class anywhere in the ontology's class
/// signature — the probe must be fresh, never silently overwriting or
/// colliding with a pre-existing class (declared, or merely referenced).
fn ensure_fresh<A: ForIRI>(onto: &SetOntology<A>, iri: &str) -> Result<(), ReasonError> {
    if class_signature(onto).contains(iri) {
        return Err(ReasonError::UnknownClass(format!(
            "probe IRI {iri} collides with a declared class"
        )));
    }
    Ok(())
}

fn probe_axiom<A: ForIRI>(build: &Build<A>, iri: &str, ce: &ClassExpression<A>) -> Component<A> {
    Component::EquivalentClasses(EquivalentClasses(vec![
        ClassExpression::Class(build.class(iri)),
        ce.clone(),
    ]))
}

/// Is `ce` satisfiable w.r.t. `onto`? Reduces to satisfiability of a fresh
/// probe class defined as `Q ≡ ce`.
///
/// # Errors
/// [`ReasonError::UnknownClass`] if the probe IRI collides with an existing
/// declared class; otherwise propagates reasoner errors.
pub fn class_expression_satisfiable<A: ForIRI>(
    onto: &SetOntology<A>,
    ce: &ClassExpression<A>,
) -> Result<CeVerdict, ReasonError> {
    // Before the probe axiom is added: an input that already declares the probe
    // IRI is refused, not extended.
    ensure_fresh(onto, PROBE_IRI)?;
    // One conversion, of the probed ontology: `Q ≡ ce` with a fresh `Q` is a
    // conservative extension, so its consistency is the input's (#369).
    let mut probed = onto.clone();
    let build: Build<A> = Build::new();
    probed.insert(probe_axiom(&build, PROBE_IRI, ce));
    let internal = owl_dl_core::convert::convert_ontology(&probed)?;
    // #313: the inconsistent-KB guard, its `ABox` saturation bounded (#428); the
    // query reuses the EL closure it builds.
    let closure = crate::bounded_query_guard(&internal, "sat-expr")?;
    let dropped = internal.dropped.clone();
    let (holds, stats, left) =
        match crate::is_class_satisfiable_keeping(internal, PROBE_IRI, closure) {
            // #462: the sound direction of no verdict is "satisfiable".
            Err(ReasonError::NoVerdict) => return Ok(CeVerdict::from_no_verdict(true, dropped)),
            r => r?,
        };
    // #369: "unsatisfiable" on a KB only the wedge finds inconsistent is vacuous.
    let lazy = crate::LazyCheck::run_if(!holds, || {
        left.lazy_inconsistency_check("class_expression_satisfiable")
    })?;
    debug_assert_eq!(
        stats.dropped_axioms,
        dropped.total(),
        "#552: one conversion"
    );
    let (incomplete, completeness_guaranteed) = verdict_flags(stats, lazy.undetermined);
    Ok(CeVerdict {
        holds,
        incomplete,
        completeness_guaranteed,
        relies_on_trusted_consistency: stats.relies_on_trusted_consistency,
        consistency_undetermined: lazy.undetermined,
        // #493: the lazy check's own probe stalled (a likely bug); the answer stands.
        no_verdict: lazy.no_verdict,
        dropped,
    })
}

/// Is `sub_ce ⊑ sup_ce` entailed by `onto`? Reduces to subsumption between two
/// fresh probe classes each defined as equivalent to one of the operands.
///
/// # Errors
/// As [`class_expression_satisfiable`].
pub fn class_expression_entailed_subclass<A: ForIRI>(
    onto: &SetOntology<A>,
    sub_ce: &ClassExpression<A>,
    sup_ce: &ClassExpression<A>,
) -> Result<CeVerdict, ReasonError> {
    ensure_fresh(onto, PROBE_IRI)?;
    ensure_fresh(onto, PROBE_IRI_2)?;
    // One conversion, of the probed ontology (see `class_expression_satisfiable`).
    let mut probed = onto.clone();
    let build: Build<A> = Build::new();
    probed.insert(probe_axiom(&build, PROBE_IRI, sub_ce));
    probed.insert(probe_axiom(&build, PROBE_IRI_2, sup_ce));
    let internal = owl_dl_core::convert::convert_ontology(&probed)?;
    // #313: the inconsistent-KB guard, as in `class_expression_satisfiable`.
    let closure = crate::bounded_query_guard(&internal, "subclass-expr")?;
    let dropped = internal.dropped.clone();
    let (holds, stats, left) =
        match crate::is_subclass_of_keeping(internal, PROBE_IRI, PROBE_IRI_2, closure) {
            // #462: the sound direction of no verdict is "not entailed".
            Err(ReasonError::NoVerdict) => return Ok(CeVerdict::from_no_verdict(false, dropped)),
            r => r?,
        };
    // #369: as in `class_expression_satisfiable`.
    let lazy = crate::LazyCheck::run_if(holds, || {
        left.lazy_inconsistency_check("class_expression_entailed_subclass")
    })?;
    debug_assert_eq!(
        stats.dropped_axioms,
        dropped.total(),
        "#552: one conversion"
    );
    let (incomplete, completeness_guaranteed) = verdict_flags(stats, lazy.undetermined);
    Ok(CeVerdict {
        holds,
        incomplete,
        completeness_guaranteed,
        relies_on_trusted_consistency: stats.relies_on_trusted_consistency,
        consistency_undetermined: lazy.undetermined,
        // #493: as in `class_expression_satisfiable`.
        no_verdict: lazy.no_verdict,
        dropped,
    })
}

/// The individuals provably in `ce` w.r.t. `onto`. Reduces to the named-class
/// instance query over a fresh probe class defined as `Q ≡ ce`; the synthetic
/// probe IRI itself is filtered out of the result.
///
/// # Errors
/// As [`class_expression_satisfiable`].
pub fn class_expression_instances<A: ForIRI>(
    onto: &SetOntology<A>,
    ce: &ClassExpression<A>,
) -> Result<CeInstances, ReasonError> {
    ensure_fresh(onto, PROBE_IRI)?;
    let mut probed = onto.clone();
    let build: Build<A> = Build::new();
    probed.insert(probe_axiom(&build, PROBE_IRI, ce));
    // Read the fragment on the probed ontology first: on it the instance query
    // takes the complete saturation fast path. (A companion `sat` probe used to
    // supply this signal; it answered a different question, so its give-up was
    // not a give-up of this query.)
    let internal = owl_dl_core::convert::convert_ontology(&probed)?;
    let fast_path = crate::realize::realize_saturation_eligible(&internal);
    // #369: the lazy consistency check runs inside the instance query.
    let (mut individuals, flags) =
        crate::realize::instances_of_reporting_internal(&internal, PROBE_IRI)?;
    individuals.retain(|i| !i.starts_with("urn:rustdl-ce-probe"));
    individuals.sort();
    individuals.dedup();
    Ok(CeInstances {
        individuals,
        incomplete: flags.incomplete,
        completeness_guaranteed: instances_guarantee(
            fast_path,
            flags,
            internal.dropped.is_empty(),
            crate::query_inconsistency_guard_enabled(),
        ),
        consistency_undetermined: flags.consistency_undetermined,
        no_verdict: flags.no_verdict,
        depth_capped: flags.depth_capped,
        dropped: internal.dropped.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::{instances_guarantee, verdict_flags};
    use crate::QueryStats;
    use crate::realize::InstanceFlags;

    /// The one state that guarantees a sat/subclass verdict.
    fn base() -> QueryStats {
        QueryStats {
            pure_el_mode: true,
            ..QueryStats::default()
        }
    }

    /// #540, #552: each condition of the sat/subclass guarantee alone voids it. The
    /// base is the one state that guarantees completeness; every row flips one input
    /// away from it. Deleting a conjunct from `QueryStats::completeness_guaranteed`
    /// (or the lazy fold in `verdict_flags`) fails its row.
    #[test]
    fn each_condition_alone_voids_the_expression_guarantee() {
        assert!(verdict_flags(base(), false).1, "base state must guarantee");
        let rows: [(&str, QueryStats, bool); 6] = [
            (
                "off the pure-EL fast path",
                QueryStats {
                    pure_el_mode: false,
                    ..base()
                },
                false,
            ),
            (
                "a probe gave up",
                QueryStats {
                    incomplete: true,
                    ..base()
                },
                false,
            ),
            (
                "conversion dropped an axiom (#540)",
                QueryStats {
                    dropped_axioms: 1,
                    ..base()
                },
                false,
            ),
            (
                "consistency rests on a trusted wedge Sat (#552)",
                QueryStats {
                    relies_on_trusted_consistency: true,
                    ..base()
                },
                false,
            ),
            (
                "consistency undetermined, from the stats (#552)",
                QueryStats {
                    consistency_undetermined: true,
                    ..base()
                },
                false,
            ),
            (
                "consistency undetermined, from the lazy check (#552)",
                base(),
                true,
            ),
        ];
        for (name, stats, lazy) in rows {
            assert!(
                !verdict_flags(stats, lazy).1,
                "{name}: this condition alone must void the guarantee"
            );
        }
    }

    /// #552: the instance query's guarantee, one condition per row.
    #[test]
    fn each_condition_alone_voids_the_instances_guarantee() {
        let ok = InstanceFlags::default();
        assert!(
            instances_guarantee(true, ok, true, true),
            "base state must guarantee"
        );
        let cut = InstanceFlags {
            incomplete: true,
            ..ok
        };
        let undetermined = InstanceFlags {
            consistency_undetermined: true,
            ..ok
        };
        for (name, args) in [
            ("off the fast path", (false, ok, true, true)),
            ("a probe was cut", (true, cut, true, true)),
            (
                "conversion dropped an axiom (#540)",
                (true, ok, false, true),
            ),
            (
                "consistency undetermined (#552)",
                (true, undetermined, true, true),
            ),
            (
                "the inconsistency guard is off (#552)",
                (true, ok, true, false),
            ),
        ] {
            assert!(
                !instances_guarantee(args.0, args.1, args.2, args.3),
                "{name}: this condition alone must void the guarantee"
            );
        }
    }
}
