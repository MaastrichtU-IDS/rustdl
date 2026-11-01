//! Black-box justification: minimal responsible-axiom sets for an entailment,
//! found by re-checking subsets of the ontology's axioms via the public
//! reasoner API. No engine internals.

use std::collections::{BTreeSet, HashSet};

use horned_owl::model::{
    Build, Class, ClassExpression, Component, DataProperty, DataPropertyAssertion,
    DifferentIndividuals, EquivalentClasses, ForIRI, Individual, Literal, MutableOntology,
    NegativeDataPropertyAssertion, NegativeObjectPropertyAssertion, ObjectProperty,
    ObjectPropertyAssertion, ObjectPropertyExpression, SameIndividual, SubObjectPropertyExpression,
};
use horned_owl::ontology::set::SetOntology;

use crate::ReasonError;
use crate::classify::{FragmentClassification, analyze_fragment};

/// An entailment to justify ("why does this hold?").
#[derive(Debug, Clone)]
pub enum Entailment {
    SubClassOf {
        sub: String,
        sup: String,
    },
    EquivalentClasses {
        a: String,
        b: String,
    },
    DisjointClasses {
        a: String,
        b: String,
    },
    Unsatisfiable {
        class: String,
    },
    InstanceOf {
        individual: String,
        class: String,
    },
    Inconsistent,
    SubObjectProperty {
        sub: String,
        sup: String,
    },
    EquivalentObjectProperties {
        a: String,
        b: String,
    },
    DisjointObjectProperties {
        a: String,
        b: String,
    },
    ObjectPropertyAssertion {
        source: String,
        prop: String,
        target: String,
    },
    SameIndividual {
        a: String,
        b: String,
    },
    DifferentIndividuals {
        a: String,
        b: String,
    },
    SubDataProperty {
        sub: String,
        sup: String,
    },
    EquivalentDataProperties {
        a: String,
        b: String,
    },
    DisjointDataProperties {
        a: String,
        b: String,
    },
    DataPropertyValue {
        source: String,
        prop: String,
        value_lexical: String,
        value_datatype: String,
    },
}

/// Parse a literal argument of the form `"lex"^^<dt>`, `"lex"^^xsd:type`, or
/// `"lex"` (bare string).
///
/// Returns `(lexical, datatype_iri)`.
#[must_use]
pub fn parse_literal_arg(s: &str) -> (String, String) {
    const XSD: &str = "http://www.w3.org/2001/XMLSchema#";
    if let Some(pos) = s.find("^^") {
        let lex_raw = &s[..pos];
        let dt_raw = &s[pos + 2..];
        // Strip surrounding double-quotes from the lexical part.
        let lex = lex_raw.trim_matches('"').to_string();
        // Strip surrounding <> (IRI form) or expand xsd: prefix.
        let dt = if dt_raw.starts_with('<') && dt_raw.ends_with('>') {
            dt_raw[1..dt_raw.len() - 1].to_string()
        } else if let Some(local) = dt_raw.strip_prefix("xsd:") {
            format!("{XSD}{local}")
        } else {
            dt_raw.to_string()
        };
        (lex, dt)
    } else {
        // No datatype: strip surrounding double-quotes, default to xsd:string.
        let lex = s.trim_matches('"').to_string();
        (lex, format!("{XSD}string"))
    }
}

/// Parse a justify/repair query (the CLI's positional `<query…>` words) into an
/// [`Entailment`].
///
/// # Errors
/// Returns a human-readable usage/error string when the query form is not
/// recognized or has the wrong arity.
pub fn parse_query(parts: &[String]) -> Result<Entailment, String> {
    let kind = parts.first().map_or("", String::as_str);
    Ok(match (kind, parts.len()) {
        ("subclass", 3) => Entailment::SubClassOf {
            sub: parts[1].clone(),
            sup: parts[2].clone(),
        },
        ("equivalent", 3) => Entailment::EquivalentClasses {
            a: parts[1].clone(),
            b: parts[2].clone(),
        },
        ("disjoint", 3) => Entailment::DisjointClasses {
            a: parts[1].clone(),
            b: parts[2].clone(),
        },
        ("unsat", 2) => Entailment::Unsatisfiable {
            class: parts[1].clone(),
        },
        ("instance", 3) => Entailment::InstanceOf {
            individual: parts[1].clone(),
            class: parts[2].clone(),
        },
        ("inconsistent", 1) => Entailment::Inconsistent,
        ("subproperty", 3) => Entailment::SubObjectProperty {
            sub: parts[1].clone(),
            sup: parts[2].clone(),
        },
        ("equiv-property", 3) => Entailment::EquivalentObjectProperties {
            a: parts[1].clone(),
            b: parts[2].clone(),
        },
        ("disjoint-property", 3) => Entailment::DisjointObjectProperties {
            a: parts[1].clone(),
            b: parts[2].clone(),
        },
        ("property", 4) => Entailment::ObjectPropertyAssertion {
            source: parts[1].clone(),
            prop: parts[2].clone(),
            target: parts[3].clone(),
        },
        ("same", 3) => Entailment::SameIndividual {
            a: parts[1].clone(),
            b: parts[2].clone(),
        },
        ("different", 3) => Entailment::DifferentIndividuals {
            a: parts[1].clone(),
            b: parts[2].clone(),
        },
        ("subdata-property", 3) => Entailment::SubDataProperty {
            sub: parts[1].clone(),
            sup: parts[2].clone(),
        },
        ("equiv-data-property", 3) => Entailment::EquivalentDataProperties {
            a: parts[1].clone(),
            b: parts[2].clone(),
        },
        ("disjoint-data-property", 3) => Entailment::DisjointDataProperties {
            a: parts[1].clone(),
            b: parts[2].clone(),
        },
        ("data-value", 4) => {
            let (value_lexical, value_datatype) = parse_literal_arg(&parts[3]);
            Entailment::DataPropertyValue {
                source: parts[1].clone(),
                prop: parts[2].clone(),
                value_lexical,
                value_datatype,
            }
        }
        _ => {
            return Err(
                "usage: justify <file> (subclass S T | equivalent A B | disjoint A B | unsat C | \
                 instance I C | inconsistent | subproperty P Q | equiv-property P Q | \
                 disjoint-property P Q | property A P B | same A B | different A B | \
                 subdata-property DP DQ | equiv-data-property DP DQ | \
                 disjoint-data-property DP DQ | data-value A DP V)"
                    .to_string(),
            );
        }
    })
}

const PROBE_IRI: &str = "urn:rustdl-justify-probe";
const PROBE_A: &str = "urn:rustdl-justify-probe-a";
const PROBE_B: &str = "urn:rustdl-justify-probe-b";
const PROBE_INT: &str = "http://www.w3.org/2001/XMLSchema#integer";

/// Diagnostic: count of oracle consistency checks issued this run (set
/// `RUSTDL_JUSTIFY_TRACE=1` to emit one stderr line per check: verdict,
/// wall-ms, and which engine answered).
static JUSTIFY_ORACLE_CALLS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn justify_trace_enabled() -> bool {
    std::env::var_os("RUSTDL_JUSTIFY_TRACE").is_some_and(|v| v == "1")
}

thread_local! {
    /// When set, the [`Entailment::Inconsistent`] oracle uses a saturation-only
    /// consistency check — a SOUND refuter (clash ⇒ genuinely inconsistent;
    /// no-clash ⇒ treated "consistent", i.e. "not refuted") — instead of the
    /// full `is_consistent`, skipping the unbounded wedge/tableau. Enabled for an
    /// `Inconsistent` justification ONLY when the full ontology's inconsistency is
    /// itself saturation-detected, so QuickXplain converges on the (valid)
    /// saturation-conflict. Soundness: with a sound refuter as the oracle, a
    /// false-"consistent" can only cost *minimality* (QX keeps extra axioms),
    /// never validity — the returned set always carries a real clash. Set
    /// `RUSTDL_JUSTIFY_SAT_ORACLE=0` to force the full-`is_consistent` oracle.
    static SATURATION_ORACLE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// RAII guard enabling [`SATURATION_ORACLE`] for its lifetime (restores prior).
struct SatOracleGuard(bool);
impl SatOracleGuard {
    fn enabled() -> Self {
        Self(SATURATION_ORACLE.with(|c| c.replace(true)))
    }
}
impl Drop for SatOracleGuard {
    fn drop(&mut self) {
        let prev = self.0;
        SATURATION_ORACLE.with(|c| c.set(prev));
    }
}

fn sat_oracle_opt_out() -> bool {
    std::env::var_os("RUSTDL_JUSTIFY_SAT_ORACLE").is_some_and(|v| v == "0")
}

/// `true` iff `onto` is inconsistent by the saturation refuter. SOUND but
/// under-approximate: a clash is a genuine inconsistency; `false` means only
/// "not refuted" (could be consistent, or inconsistent outside the saturation
/// fragment) — never a consistency proof.
fn saturation_inconsistent<A: ForIRI>(onto: &SetOntology<A>) -> bool {
    owl_dl_core::convert::convert_ontology(onto)
        .is_ok_and(|internal| crate::abox_saturation::saturate_abox_consistency(&internal).clash)
}

/// Enable the saturation-only oracle for an `Inconsistent` justification when the
/// full candidate set is saturation-inconsistent (one convert+saturate, ~1.6s).
/// Returns `None` (full-`is_consistent` oracle) otherwise — so non-saturation
/// inconsistencies keep the complete behavior.
fn maybe_saturation_oracle<A: ForIRI>(
    fixed: &[Component<A>],
    all_candidates: &[Component<A>],
    q: &Entailment,
) -> Option<SatOracleGuard> {
    if sat_oracle_opt_out() || !matches!(q, Entailment::Inconsistent) {
        return None;
    }
    if saturation_inconsistent(&ontology_from(fixed, all_candidates)) {
        if justify_trace_enabled() {
            eprintln!(
                "[justify-trace] saturation oracle ENABLED (full inconsistency is saturation-detected)"
            );
        }
        Some(SatOracleGuard::enabled())
    } else {
        None
    }
}

/// A three-valued entailment verdict (#396).
///
/// The reasoner checks under [`entails`] report a GIVE-UP (the live-node cap
/// `RUSTDL_MAX_NODES`, a cut instance probe, a consistency fallback deadline) in
/// its sound direction, which for every [`Entailment`] kind is "not entailed".
/// `Unknown` is that give-up made visible: no entailment was witnessed, but none
/// was refuted either. `Entailed` and `NotEntailed` are definite.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// The entailment holds (witnessed).
    Entailed,
    /// The entailment does not hold (refuted, with no give-up).
    NotEntailed,
    /// A check gave up before deciding.
    Unknown,
}

impl Verdict {
    /// The verdict of a check answering `holds`, with `gave_up` its give-up flag.
    /// A positive is a witness even when part of the check was cut, so only a
    /// negative is called into question.
    fn from_check(holds: bool, gave_up: bool) -> Self {
        match (holds, gave_up) {
            (true, _) => Self::Entailed,
            (false, false) => Self::NotEntailed,
            (false, true) => Self::Unknown,
        }
    }

    /// The verdict of `self AND second()`: a definite `NotEntailed` on either side
    /// decides, `Entailed` needs both. `second` still runs when `self` is
    /// `Unknown`, since it may be a definite no.
    fn and(self, second: impl FnOnce() -> Result<Self, ReasonError>) -> Result<Self, ReasonError> {
        if self == Self::NotEntailed {
            return Ok(self);
        }
        Ok(match (self, second()?) {
            (_, Self::NotEntailed) => Self::NotEntailed,
            (Self::Entailed, Self::Entailed) => Self::Entailed,
            _ => Self::Unknown,
        })
    }
}

/// Does `onto` entail `q`? `true` only for [`Verdict::Entailed`]: a give-up
/// reads as "not entailed", in its sound direction. Callers that present the
/// answer as a non-entailment should use [`entails_reporting`] instead.
///
/// # Errors
/// Propagates [`ReasonError`] from the underlying reasoner.
pub fn entails<A: ForIRI>(onto: &SetOntology<A>, q: &Entailment) -> Result<bool, ReasonError> {
    Ok(entails_reporting(onto, q)? == Verdict::Entailed)
}

/// Does `onto` entail `q`, as a [`Verdict`]? Reduces to the reporting forms of
/// the public reasoner checks. The `DisjointClasses` case injects a fresh probe
/// class `X ≡ a ⊓ b` and checks `X` unsatisfiable (probe = query encoding; never
/// part of a justification).
///
/// # Errors
/// Propagates [`ReasonError`] from the underlying reasoner.
pub fn entails_reporting<A: ForIRI>(
    onto: &SetOntology<A>,
    q: &Entailment,
) -> Result<Verdict, ReasonError> {
    let subclass = |sub: &str, sup: &str| -> Result<Verdict, ReasonError> {
        let (holds, stats) = crate::is_subclass_of_with_stats(onto, sub, sup)?;
        Ok(Verdict::from_check(holds, stats.incomplete))
    };
    match q {
        Entailment::SubClassOf { sub, sup } => subclass(sub, sup),
        Entailment::EquivalentClasses { a, b } => subclass(a, b)?.and(|| subclass(b, a)),
        Entailment::DisjointClasses { a, b } => {
            let mut probed = onto.clone();
            let build: Build<A> = Build::new();
            probed.insert(Component::EquivalentClasses(EquivalentClasses(vec![
                ClassExpression::Class(build.class(PROBE_IRI)),
                ClassExpression::ObjectIntersectionOf(vec![
                    ClassExpression::Class(build.class(a.as_str())),
                    ClassExpression::Class(build.class(b.as_str())),
                ]),
            ])));
            let (sat, stats) = crate::is_class_satisfiable_with_stats(&probed, PROBE_IRI)?;
            Ok(Verdict::from_check(!sat, stats.incomplete))
        }
        Entailment::Unsatisfiable { class } => {
            let (sat, stats) = crate::is_class_satisfiable_with_stats(onto, class)?;
            Ok(Verdict::from_check(!sat, stats.incomplete))
        }
        // is_instance_of is (class, individual) — class first.
        Entailment::InstanceOf { individual, class } => {
            match crate::is_instance_of_reporting(onto, class, individual) {
                // Inconsistent ⇒ everything is entailed, as for property assertions above;
                // QuickXplain then returns the inconsistency core (#313).
                Err(ReasonError::Inconsistent) => Ok(Verdict::Entailed),
                r => r.map(|(holds, cut)| Verdict::from_check(holds, cut)),
            }
        }
        Entailment::Inconsistent => {
            let sat_mode = SATURATION_ORACLE.with(std::cell::Cell::get);
            if justify_trace_enabled() {
                let n = JUSTIFY_ORACLE_CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
                let t = std::time::Instant::now();
                // The saturation-only oracle is a sound refuter by design (see
                // [`SATURATION_ORACLE`]), not a give-up: its "not refuted" can cost
                // minimality only, so it is not reported as `Unknown`.
                let (inconsistent, gave_up, engine) = if sat_mode {
                    (saturation_inconsistent(onto), false, "saturation-only")
                } else {
                    let (consistent, stats) = crate::is_consistent_with_stats(onto)?;
                    let engine = if stats.answered_by_saturation {
                        "saturation"
                    } else {
                        "wedge/tableau"
                    };
                    (!consistent, stats.incomplete, engine)
                };
                eprintln!(
                    "[justify-trace] call#{n} verdict={} {}ms engine={engine} axioms={}",
                    if inconsistent {
                        "INCONSISTENT"
                    } else {
                        "consistent"
                    },
                    t.elapsed().as_millis(),
                    onto.iter().count()
                );
                Ok(Verdict::from_check(inconsistent, gave_up))
            } else if sat_mode {
                Ok(Verdict::from_check(saturation_inconsistent(onto), false))
            } else {
                let (consistent, stats) = crate::is_consistent_with_stats(onto)?;
                Ok(Verdict::from_check(!consistent, stats.incomplete))
            }
        }
        Entailment::SubObjectProperty { sub, sup } => {
            let b: Build<A> = Build::new();
            inconsistent_with(
                onto,
                vec![
                    Component::ObjectPropertyAssertion(ObjectPropertyAssertion {
                        ope: ope(&b, sub),
                        from: named(&b, PROBE_A),
                        to: named(&b, PROBE_B),
                    }),
                    Component::NegativeObjectPropertyAssertion(NegativeObjectPropertyAssertion {
                        ope: ope(&b, sup),
                        from: named(&b, PROBE_A),
                        to: named(&b, PROBE_B),
                    }),
                ],
            )
        }
        Entailment::EquivalentObjectProperties { a, b } => entails_reporting(
            onto,
            &Entailment::SubObjectProperty {
                sub: a.clone(),
                sup: b.clone(),
            },
        )?
        .and(|| {
            entails_reporting(
                onto,
                &Entailment::SubObjectProperty {
                    sub: b.clone(),
                    sup: a.clone(),
                },
            )
        }),
        Entailment::DisjointObjectProperties { a, b } => {
            let bld: Build<A> = Build::new();
            inconsistent_with(
                onto,
                vec![
                    Component::ObjectPropertyAssertion(ObjectPropertyAssertion {
                        ope: ope(&bld, a),
                        from: named(&bld, PROBE_A),
                        to: named(&bld, PROBE_B),
                    }),
                    Component::ObjectPropertyAssertion(ObjectPropertyAssertion {
                        ope: ope(&bld, b),
                        from: named(&bld, PROBE_A),
                        to: named(&bld, PROBE_B),
                    }),
                ],
            )
        }
        Entailment::ObjectPropertyAssertion {
            source,
            prop,
            target,
        } => {
            // Union of two sound, monotone oracles (soundness by composition):
            //   (1) the RBox-complete named-individual edge set from
            //       `materialize_object_property_assertions` (transitivity /
            //       symmetry / inverse / SameIndividual folding / ObjectHasValue),
            //       validated FP-free against the HermiT oracle in #26; and
            //   (2) the NegOPA inconsistency probe, which still covers edges
            //       outside that fragment (class-axiom / tableau-derived).
            // The NegOPA probe alone was incomplete for (1) — see issue #28.
            match crate::materialize_object_property_assertions(onto) {
                Ok(edges) => {
                    if edges
                        .iter()
                        .any(|(s, p, t)| s == source && p == prop && t == target)
                    {
                        return Ok(Verdict::Entailed);
                    }
                }
                // Inconsistent ⇒ everything is entailed (matches the probe below).
                Err(ReasonError::Inconsistent) => return Ok(Verdict::Entailed),
                Err(e) => return Err(e),
            }
            let b: Build<A> = Build::new();
            inconsistent_with(
                onto,
                vec![Component::NegativeObjectPropertyAssertion(
                    NegativeObjectPropertyAssertion {
                        ope: ope(&b, prop),
                        from: named(&b, source),
                        to: named(&b, target),
                    },
                )],
            )
        }
        Entailment::SameIndividual { a, b } => {
            let bld: Build<A> = Build::new();
            inconsistent_with(
                onto,
                vec![Component::DifferentIndividuals(DifferentIndividuals(vec![
                    named(&bld, a),
                    named(&bld, b),
                ]))],
            )
        }
        Entailment::DifferentIndividuals { a, b } => {
            let bld: Build<A> = Build::new();
            inconsistent_with(
                onto,
                vec![Component::SameIndividual(SameIndividual(vec![
                    named(&bld, a),
                    named(&bld, b),
                ]))],
            )
        }
        Entailment::SubDataProperty { sub, sup } => {
            let b: Build<A> = Build::new();
            let sub_lit = Literal::Datatype {
                literal: "0".to_string(),
                datatype_iri: b.iri(PROBE_INT),
            };
            let sup_lit = Literal::Datatype {
                literal: "0".to_string(),
                datatype_iri: b.iri(PROBE_INT),
            };
            // c1: asserting sub(_a,0) alone must be consistent (else range clash, not subsumption).
            let guard = inconsistent_with(
                onto,
                vec![Component::DataPropertyAssertion(DataPropertyAssertion {
                    dp: b.data_property(sub.as_str()),
                    from: named(&b, PROBE_A),
                    to: sub_lit,
                })],
            )?;
            if guard == Verdict::Entailed {
                return Ok(Verdict::NotEntailed);
            }
            // c2: adding ¬sup(_a,0) becomes inconsistent ⟺ sub⊑sup forces sup(_a,0).
            let main = inconsistent_with(
                onto,
                vec![
                    Component::DataPropertyAssertion(DataPropertyAssertion {
                        dp: b.data_property(sub.as_str()),
                        from: named(&b, PROBE_A),
                        to: Literal::Datatype {
                            literal: "0".to_string(),
                            datatype_iri: b.iri(PROBE_INT),
                        },
                    }),
                    Component::NegativeDataPropertyAssertion(NegativeDataPropertyAssertion {
                        dp: b.data_property(sup.as_str()),
                        from: named(&b, PROBE_A),
                        to: sup_lit,
                    }),
                ],
            )?;
            Ok(guarded(guard == Verdict::Unknown, main))
        }
        Entailment::EquivalentDataProperties { a, b } => entails_reporting(
            onto,
            &Entailment::SubDataProperty {
                sub: a.clone(),
                sup: b.clone(),
            },
        )?
        .and(|| {
            entails_reporting(
                onto,
                &Entailment::SubDataProperty {
                    sub: b.clone(),
                    sup: a.clone(),
                },
            )
        }),
        Entailment::DisjointDataProperties { a, b } => {
            // Reduce to inconsistency: assert dp_a(_probe, 0^^xsd:integer) and
            // dp_b(_probe, 0^^xsd:integer) — if `DisjointDataProperties(a,b)` holds
            // the ABox is inconsistent (same value on disjoint properties).
            //
            // Guard (c1/c2): first verify that asserting each property alone is
            // consistent — if a property has a range restriction excluding
            // xsd:integer, the single assertion would already be inconsistent
            // (a DataPropertyRange clash, not a disjointness entailment).
            // Mirror the SubDataProperty c1 guard.
            let bld: Build<A> = Build::new();
            let make_lit = || Literal::Datatype {
                literal: "0".to_string(),
                datatype_iri: bld.iri(PROBE_INT),
            };
            // c1a: assert a(probe, 0) alone — must be consistent.
            let guard_a = inconsistent_with(
                onto,
                vec![Component::DataPropertyAssertion(DataPropertyAssertion {
                    dp: bld.data_property(a.as_str()),
                    from: named(&bld, PROBE_A),
                    to: make_lit(),
                })],
            )?;
            if guard_a == Verdict::Entailed {
                return Ok(Verdict::NotEntailed);
            }
            // c1b: assert b(probe, 0) alone — must be consistent.
            let guard_b = inconsistent_with(
                onto,
                vec![Component::DataPropertyAssertion(DataPropertyAssertion {
                    dp: bld.data_property(b.as_str()),
                    from: named(&bld, PROBE_A),
                    to: make_lit(),
                })],
            )?;
            if guard_b == Verdict::Entailed {
                return Ok(Verdict::NotEntailed);
            }
            // c2: asserting both a(probe,0) and b(probe,0) ⟹ inconsistent iff
            // DisjointDataProperties(a,b) is entailed.
            let main = inconsistent_with(
                onto,
                vec![
                    Component::DataPropertyAssertion(DataPropertyAssertion {
                        dp: bld.data_property(a.as_str()),
                        from: named(&bld, PROBE_A),
                        to: make_lit(),
                    }),
                    Component::DataPropertyAssertion(DataPropertyAssertion {
                        dp: bld.data_property(b.as_str()),
                        from: named(&bld, PROBE_A),
                        to: make_lit(),
                    }),
                ],
            )?;
            Ok(guarded(
                guard_a == Verdict::Unknown || guard_b == Verdict::Unknown,
                main,
            ))
        }
        Entailment::DataPropertyValue {
            source,
            prop,
            value_lexical,
            value_datatype,
        } => {
            let b: Build<A> = Build::new();
            let lit = Literal::Datatype {
                literal: value_lexical.clone(),
                datatype_iri: b.iri(value_datatype.as_str()),
            };
            inconsistent_with(
                onto,
                vec![Component::NegativeDataPropertyAssertion(
                    NegativeDataPropertyAssertion {
                        dp: b.data_property(prop.as_str()),
                        from: named(&b, source),
                        to: lit,
                    },
                )],
            )
        }
    }
}

/// The verdict of a data-property probe `main` behind its consistency guard
/// (#396). The guard rules out a range clash: if asserting the property alone is
/// already inconsistent, the probe answers "not entailed". A guard that GAVE UP
/// ruled nothing out, so `main`'s "inconsistent" may be that very clash, not the
/// entailment: it must not read as `Entailed`. A definite "no" from `main` still
/// decides, since `main` only adds axioms to the guard's ontology.
fn guarded(guard_gave_up: bool, main: Verdict) -> Verdict {
    if guard_gave_up && main == Verdict::Entailed {
        Verdict::Unknown
    } else {
        main
    }
}

/// Is `onto ∪ extra` inconsistent? `Entailed` iff it is; `Unknown` when the
/// consistency check gave up (reported "consistent" without a model at the usual
/// trust level). The `extra` axioms are query-encoding probes (fresh `PROBE_*`
/// symbols), never candidate axioms — they appear in every tested subset and
/// never in a justification.
fn inconsistent_with<A: ForIRI>(
    onto: &SetOntology<A>,
    extra: Vec<Component<A>>,
) -> Result<Verdict, ReasonError> {
    let mut probed = onto.clone();
    for c in extra {
        probed.insert(c);
    }
    let (consistent, stats) = crate::is_consistent_with_stats(&probed)?;
    Ok(Verdict::from_check(!consistent, stats.incomplete))
}

fn named<A: ForIRI>(b: &Build<A>, iri: &str) -> Individual<A> {
    Individual::Named(b.named_individual(iri))
}

fn ope<A: ForIRI>(b: &Build<A>, iri: &str) -> ObjectPropertyExpression<A> {
    ObjectPropertyExpression::ObjectProperty(b.object_property(iri))
}

/// Split `onto` into (`fixed`, `candidates`): `fixed` = non-logical axioms
/// (declarations / annotations / metadata) retained in every tested ontology;
/// `candidates` = logical axioms, the only possible justification members.
#[must_use]
pub fn logical_axioms<A: ForIRI>(onto: &SetOntology<A>) -> (Vec<Component<A>>, Vec<Component<A>>) {
    let mut fixed = Vec::new();
    let mut candidates = Vec::new();
    for ac in onto {
        let c = ac.component.clone();
        if is_logical(&c) {
            candidates.push(c);
        } else {
            fixed.push(c);
        }
    }
    (fixed, candidates)
}

/// A logical axiom can affect entailment and may appear in a justification;
/// declarations / annotations / ontology metadata cannot.
fn is_logical<A: ForIRI>(c: &Component<A>) -> bool {
    !matches!(
        c,
        Component::OntologyID(_)
            | Component::DocIRI(_)
            | Component::Import(_)
            | Component::OntologyAnnotation(_)
            | Component::DeclareClass(_)
            | Component::DeclareObjectProperty(_)
            | Component::DeclareAnnotationProperty(_)
            | Component::DeclareDataProperty(_)
            | Component::DeclareNamedIndividual(_)
            | Component::DeclareDatatype(_)
            | Component::AnnotationAssertion(_)
            | Component::SubAnnotationPropertyOf(_)
            | Component::AnnotationPropertyDomain(_)
            | Component::AnnotationPropertyRange(_)
    )
}

/// Build a `SetOntology` from `fixed` + the candidate `subset`.
#[must_use]
pub fn ontology_from<A: ForIRI>(fixed: &[Component<A>], subset: &[Component<A>]) -> SetOntology<A> {
    let mut o = SetOntology::new();
    for c in fixed.iter().chain(subset.iter()) {
        o.insert(c.clone());
    }
    o
}

const OWL_THING: &str = "http://www.w3.org/2002/07/owl#Thing";
const OWL_NOTHING: &str = "http://www.w3.org/2002/07/owl#Nothing";

fn op_iri<A: ForIRI>(p: &ObjectProperty<A>) -> &str {
    p.0.as_ref()
}
fn dp_iri<A: ForIRI>(p: &DataProperty<A>) -> &str {
    p.0.as_ref()
}
fn cls_iri<A: ForIRI>(c: &Class<A>) -> &str {
    c.0.as_ref()
}

/// The underlying property IRI of an object-property expression. `Inv(R)` is
/// keyed by `R` — for ⊥-locality, `Inv(R)` is the empty role iff `R` is.
fn ope_iri<A: ForIRI>(ope: &ObjectPropertyExpression<A>) -> &str {
    match ope {
        ObjectPropertyExpression::ObjectProperty(p)
        | ObjectPropertyExpression::InverseObjectProperty(p) => p.0.as_ref(),
    }
}

fn ind_iri<A: ForIRI>(i: &Individual<A>) -> Option<&str> {
    match i {
        Individual::Named(n) => Some(n.0.as_ref()),
        Individual::Anonymous(_) => None,
    }
}

/// Is `ce` provably ⊥ ("equivalent to owl:Nothing") under ⊥-locality w.r.t.
/// `sig`? External class names (∉ `sig`) and external roles map to ⊥/empty.
/// Returns `true` only when provably ⊥; unknown constructs return `false`.
fn ce_is_bot<A: ForIRI>(ce: &ClassExpression<A>, sig: &HashSet<String>) -> bool {
    use ClassExpression as CE;
    match ce {
        CE::Class(c) => {
            let iri = c.0.as_ref();
            iri == OWL_NOTHING || (iri != OWL_THING && !sig.contains(iri))
        }
        CE::ObjectComplementOf(c) => ce_is_top(c, sig),
        CE::ObjectIntersectionOf(cs) => cs.iter().any(|c| ce_is_bot(c, sig)),
        CE::ObjectUnionOf(cs) => !cs.is_empty() && cs.iter().all(|c| ce_is_bot(c, sig)),
        CE::ObjectSomeValuesFrom { ope, bce } => !sig.contains(ope_iri(ope)) || ce_is_bot(bce, sig),
        CE::ObjectHasValue { ope, .. } | CE::ObjectHasSelf(ope) => !sig.contains(ope_iri(ope)),
        CE::ObjectMinCardinality { n, ope, bce } | CE::ObjectExactCardinality { n, ope, bce } => {
            *n >= 1 && (!sig.contains(ope_iri(ope)) || ce_is_bot(bce, sig))
        }
        CE::ObjectOneOf(inds) => inds.is_empty(),
        CE::DataSomeValuesFrom { dp, .. } | CE::DataHasValue { dp, .. } => {
            !sig.contains(dp.0.as_ref())
        }
        CE::DataMinCardinality { n, dp, .. } | CE::DataExactCardinality { n, dp, .. } => {
            *n >= 1 && !sig.contains(dp.0.as_ref())
        }
        _ => false,
    }
}

/// Is `ce` provably ⊤ ("equivalent to owl:Thing") under ⊥-locality w.r.t.
/// `sig`? Returns `true` only when provably ⊤; unknown constructs return
/// `false`.
fn ce_is_top<A: ForIRI>(ce: &ClassExpression<A>, sig: &HashSet<String>) -> bool {
    use ClassExpression as CE;
    match ce {
        CE::Class(c) => c.0.as_ref() == OWL_THING,
        CE::ObjectComplementOf(c) => ce_is_bot(c, sig),
        CE::ObjectIntersectionOf(cs) => cs.iter().all(|c| ce_is_top(c, sig)),
        CE::ObjectUnionOf(cs) => cs.iter().any(|c| ce_is_top(c, sig)),
        CE::ObjectAllValuesFrom { ope, bce } => !sig.contains(ope_iri(ope)) || ce_is_top(bce, sig),
        CE::ObjectMaxCardinality { ope, bce, .. } => {
            !sig.contains(ope_iri(ope)) || ce_is_bot(bce, sig)
        }
        CE::ObjectMinCardinality { n, .. } | CE::DataMinCardinality { n, .. } => *n == 0,
        CE::ObjectExactCardinality { n, ope, bce } => {
            *n == 0 && (!sig.contains(ope_iri(ope)) || ce_is_bot(bce, sig))
        }
        CE::DataAllValuesFrom { dp, .. } | CE::DataMaxCardinality { dp, .. } => {
            !sig.contains(dp.0.as_ref())
        }
        _ => false,
    }
}

/// Is the sub-side of a property inclusion provably the empty role under
/// ⊥-locality? A chain is empty iff any component is external.
fn sub_ope_is_bot<A: ForIRI>(sub: &SubObjectPropertyExpression<A>, sig: &HashSet<String>) -> bool {
    match sub {
        SubObjectPropertyExpression::ObjectPropertyExpression(ope) => !sig.contains(ope_iri(ope)),
        SubObjectPropertyExpression::ObjectPropertyChain(chain) => {
            !chain.is_empty() && chain.iter().any(|ope| !sig.contains(ope_iri(ope)))
        }
    }
}

/// Is `c` ⊥-local w.r.t. `sig`? A ⊥-local axiom becomes a tautology when every
/// term outside `sig` is replaced by ⊥/empty, so it cannot belong to any
/// justification over `sig`. **Conservative: returns `true` only when provably
/// local; every unhandled construct returns `false` (non-local ⇒ kept).** This
/// under-approximates locality, so [`extract_bot_module`] yields a *superset*
/// of the true ⊥-module and never drops a justification axiom.
// Arms share bodies but cannot be merged: each `Component` variant wraps a
// distinct newtype, so an or-pattern binding `a` would fail to typecheck.
#[allow(clippy::match_same_arms)]
fn is_bot_local<A: ForIRI>(c: &Component<A>, sig: &HashSet<String>) -> bool {
    use Component as C;
    match c {
        C::SubClassOf(a) => ce_is_bot(&a.sub, sig) || ce_is_top(&a.sup, sig),
        C::EquivalentClasses(a) => {
            a.0.iter().all(|c| ce_is_bot(c, sig)) || a.0.iter().all(|c| ce_is_top(c, sig))
        }
        // Disjoint holds vacuously once at most one disjunct is non-empty.
        C::DisjointClasses(a) => a.0.iter().filter(|c| !ce_is_bot(c, sig)).count() <= 1,
        C::SubObjectPropertyOf(a) => sub_ope_is_bot(&a.sub, sig),
        C::ObjectPropertyDomain(a) => !sig.contains(ope_iri(&a.ope)) || ce_is_top(&a.ce, sig),
        C::ObjectPropertyRange(a) => !sig.contains(ope_iri(&a.ope)) || ce_is_top(&a.ce, sig),
        C::DataPropertyDomain(a) => !sig.contains(dp_iri(&a.dp)) || ce_is_top(&a.ce, sig),
        C::FunctionalObjectProperty(a) => !sig.contains(ope_iri(&a.0)),
        C::InverseFunctionalObjectProperty(a) => !sig.contains(ope_iri(&a.0)),
        C::TransitiveObjectProperty(a) => !sig.contains(ope_iri(&a.0)),
        C::SymmetricObjectProperty(a) => !sig.contains(ope_iri(&a.0)),
        C::AsymmetricObjectProperty(a) => !sig.contains(ope_iri(&a.0)),
        C::IrreflexiveObjectProperty(a) => !sig.contains(ope_iri(&a.0)),
        C::FunctionalDataProperty(a) => !sig.contains(dp_iri(&a.0)),
        C::DataPropertyRange(a) => !sig.contains(dp_iri(&a.dp)),
        C::SubDataPropertyOf(a) => !sig.contains(dp_iri(&a.sub)),
        C::InverseObjectProperties(a) => !sig.contains(op_iri(&a.0)) && !sig.contains(op_iri(&a.1)),
        C::EquivalentObjectProperties(a) => a.0.iter().all(|p| !sig.contains(ope_iri(p))),
        C::DisjointObjectProperties(a) => {
            a.0.iter().filter(|p| sig.contains(ope_iri(p))).count() <= 1
        }
        C::EquivalentDataProperties(a) => a.0.iter().all(|p| !sig.contains(dp_iri(p))),
        C::DisjointDataProperties(a) => a.0.iter().filter(|p| sig.contains(dp_iri(p))).count() <= 1,
        // ReflexiveObjectProperty (⊤ ⊑ ∃R.Self), ABox assertions, HasKey,
        // DisjointUnion, datatype defs, and anything else: NOT provably local.
        _ => false,
    }
}

/// Add every named-entity IRI occurring in `ce` to `out`.
fn collect_ce_entities<A: ForIRI>(ce: &ClassExpression<A>, out: &mut HashSet<String>) {
    use ClassExpression as CE;
    match ce {
        CE::Class(c) => {
            out.insert(c.0.as_ref().to_string());
        }
        CE::ObjectComplementOf(c) => collect_ce_entities(c, out),
        CE::ObjectIntersectionOf(cs) | CE::ObjectUnionOf(cs) => {
            for c in cs {
                collect_ce_entities(c, out);
            }
        }
        CE::ObjectSomeValuesFrom { ope, bce }
        | CE::ObjectAllValuesFrom { ope, bce }
        | CE::ObjectMinCardinality { ope, bce, .. }
        | CE::ObjectMaxCardinality { ope, bce, .. }
        | CE::ObjectExactCardinality { ope, bce, .. } => {
            out.insert(ope_iri(ope).to_string());
            collect_ce_entities(bce, out);
        }
        CE::ObjectHasValue { ope, i } => {
            out.insert(ope_iri(ope).to_string());
            if let Some(s) = ind_iri(i) {
                out.insert(s.to_string());
            }
        }
        CE::ObjectHasSelf(ope) => {
            out.insert(ope_iri(ope).to_string());
        }
        CE::ObjectOneOf(inds) => {
            for i in inds {
                if let Some(s) = ind_iri(i) {
                    out.insert(s.to_string());
                }
            }
        }
        CE::DataSomeValuesFrom { dp, .. }
        | CE::DataAllValuesFrom { dp, .. }
        | CE::DataHasValue { dp, .. }
        | CE::DataMinCardinality { dp, .. }
        | CE::DataMaxCardinality { dp, .. }
        | CE::DataExactCardinality { dp, .. } => {
            out.insert(dp.0.as_ref().to_string());
        }
    }
}

/// Add every named-entity IRI occurring in `c` to `out`. Covers all logical
/// axiom shapes; unhandled shapes contribute nothing (safe — the axiom is still
/// kept, only the signature does not grow from it).
// Arms share bodies but cannot be merged (distinct per-variant newtypes).
#[allow(clippy::match_same_arms)]
fn collect_component_entities<A: ForIRI>(c: &Component<A>, out: &mut HashSet<String>) {
    use Component as C;
    macro_rules! add {
        ($iri:expr) => {{
            out.insert(($iri).to_string());
        }};
    }
    match c {
        C::SubClassOf(a) => {
            collect_ce_entities(&a.sub, out);
            collect_ce_entities(&a.sup, out);
        }
        C::EquivalentClasses(a) => {
            for x in &a.0 {
                collect_ce_entities(x, out);
            }
        }
        C::DisjointClasses(a) => {
            for x in &a.0 {
                collect_ce_entities(x, out);
            }
        }
        C::DisjointUnion(a) => {
            add!(cls_iri(&a.0));
            for x in &a.1 {
                collect_ce_entities(x, out);
            }
        }
        C::SubObjectPropertyOf(a) => {
            add!(ope_iri(&a.sup));
            match &a.sub {
                SubObjectPropertyExpression::ObjectPropertyExpression(ope) => add!(ope_iri(ope)),
                SubObjectPropertyExpression::ObjectPropertyChain(chain) => {
                    for ope in chain {
                        add!(ope_iri(ope));
                    }
                }
            }
        }
        C::EquivalentObjectProperties(a) => {
            for p in &a.0 {
                add!(ope_iri(p));
            }
        }
        C::DisjointObjectProperties(a) => {
            for p in &a.0 {
                add!(ope_iri(p));
            }
        }
        C::InverseObjectProperties(a) => {
            add!(op_iri(&a.0));
            add!(op_iri(&a.1));
        }
        C::ObjectPropertyDomain(a) => {
            add!(ope_iri(&a.ope));
            collect_ce_entities(&a.ce, out);
        }
        C::ObjectPropertyRange(a) => {
            add!(ope_iri(&a.ope));
            collect_ce_entities(&a.ce, out);
        }
        C::FunctionalObjectProperty(a) => add!(ope_iri(&a.0)),
        C::InverseFunctionalObjectProperty(a) => add!(ope_iri(&a.0)),
        C::TransitiveObjectProperty(a) => add!(ope_iri(&a.0)),
        C::SymmetricObjectProperty(a) => add!(ope_iri(&a.0)),
        C::AsymmetricObjectProperty(a) => add!(ope_iri(&a.0)),
        C::ReflexiveObjectProperty(a) => add!(ope_iri(&a.0)),
        C::IrreflexiveObjectProperty(a) => add!(ope_iri(&a.0)),
        C::SubDataPropertyOf(a) => {
            add!(dp_iri(&a.sup));
            add!(dp_iri(&a.sub));
        }
        C::EquivalentDataProperties(a) => {
            for p in &a.0 {
                add!(dp_iri(p));
            }
        }
        C::DisjointDataProperties(a) => {
            for p in &a.0 {
                add!(dp_iri(p));
            }
        }
        C::DataPropertyDomain(a) => {
            add!(dp_iri(&a.dp));
            collect_ce_entities(&a.ce, out);
        }
        C::DataPropertyRange(a) => add!(dp_iri(&a.dp)),
        C::FunctionalDataProperty(a) => add!(dp_iri(&a.0)),
        C::HasKey(a) => collect_ce_entities(&a.ce, out),
        C::ClassAssertion(a) => {
            collect_ce_entities(&a.ce, out);
            if let Some(s) = ind_iri(&a.i) {
                add!(s);
            }
        }
        C::ObjectPropertyAssertion(a) => {
            add!(ope_iri(&a.ope));
            for i in [&a.from, &a.to] {
                if let Some(s) = ind_iri(i) {
                    add!(s);
                }
            }
        }
        C::NegativeObjectPropertyAssertion(a) => {
            add!(ope_iri(&a.ope));
            for i in [&a.from, &a.to] {
                if let Some(s) = ind_iri(i) {
                    add!(s);
                }
            }
        }
        C::DataPropertyAssertion(a) => {
            add!(dp_iri(&a.dp));
            if let Some(s) = ind_iri(&a.from) {
                add!(s);
            }
        }
        C::NegativeDataPropertyAssertion(a) => {
            add!(dp_iri(&a.dp));
            if let Some(s) = ind_iri(&a.from) {
                add!(s);
            }
        }
        C::SameIndividual(a) => {
            for i in &a.0 {
                if let Some(s) = ind_iri(i) {
                    add!(s);
                }
            }
        }
        C::DifferentIndividuals(a) => {
            for i in &a.0 {
                if let Some(s) = ind_iri(i) {
                    add!(s);
                }
            }
        }
        _ => {}
    }
}

/// All named-entity IRIs (classes, object/data properties, named individuals)
/// occurring in `c`. Exposed for callers that gloss an axiom by entity — e.g.
/// the CLI's `--labels` rendering. Deterministic order.
#[must_use]
pub fn component_entities<A: ForIRI>(c: &Component<A>) -> BTreeSet<String> {
    let mut s = HashSet::new();
    collect_component_entities(c, &mut s);
    s.into_iter().collect()
}

/// The seed signature of a query: the entity IRIs the justification is "about".
/// `None` ⇒ the query is not localizable (e.g. global inconsistency); the
/// caller keeps the full axiom set.
fn query_seed_signature(q: &Entailment) -> Option<HashSet<String>> {
    let mut s = HashSet::new();
    match q {
        Entailment::SubClassOf { sub, sup }
        | Entailment::SubObjectProperty { sub, sup }
        | Entailment::SubDataProperty { sub, sup } => {
            s.insert(sub.clone());
            s.insert(sup.clone());
        }
        Entailment::EquivalentClasses { a, b }
        | Entailment::DisjointClasses { a, b }
        | Entailment::EquivalentObjectProperties { a, b }
        | Entailment::DisjointObjectProperties { a, b }
        | Entailment::SameIndividual { a, b }
        | Entailment::DifferentIndividuals { a, b }
        | Entailment::EquivalentDataProperties { a, b }
        | Entailment::DisjointDataProperties { a, b } => {
            s.insert(a.clone());
            s.insert(b.clone());
        }
        Entailment::Unsatisfiable { class } => {
            s.insert(class.clone());
        }
        Entailment::InstanceOf { individual, class } => {
            s.insert(individual.clone());
            s.insert(class.clone());
        }
        Entailment::ObjectPropertyAssertion {
            source,
            prop,
            target,
        } => {
            s.insert(source.clone());
            s.insert(prop.clone());
            s.insert(target.clone());
        }
        Entailment::DataPropertyValue { source, prop, .. } => {
            s.insert(source.clone());
            s.insert(prop.clone());
        }
        Entailment::Inconsistent => return None,
    }
    Some(s)
}

/// Extract the syntactic ⊥-locality module of `candidates` for seed signature
/// `seed`: the fixpoint of "keep every axiom that is not ⊥-local w.r.t. the
/// signature accumulated so far, then grow the signature by that axiom's
/// terms." Justification-preserving — every justification of an entailment over
/// `seed` lies within the returned module.
#[must_use]
#[allow(clippy::implicit_hasher)] // seed is always a default-hasher HashSet here
pub fn extract_bot_module<A: ForIRI>(
    candidates: &[Component<A>],
    seed: &HashSet<String>,
) -> Vec<Component<A>> {
    let mut sig = seed.clone();
    let mut in_module = vec![false; candidates.len()];
    let mut changed = true;
    while changed {
        changed = false;
        for (i, ax) in candidates.iter().enumerate() {
            if in_module[i] || is_bot_local(ax, &sig) {
                continue;
            }
            in_module[i] = true;
            collect_component_entities(ax, &mut sig);
            changed = true;
        }
    }
    candidates
        .iter()
        .zip(&in_module)
        .filter(|(_, keep)| **keep)
        .map(|(ax, _)| ax.clone())
        .collect()
}

/// Decide entailment and pick the candidate set to search, reasoning over the
/// query's ⊥-module instead of the whole ontology whenever possible.
///
/// Returns `Ok(None)` when `q` is not entailed, else `Ok(Some(candidates))`.
/// The ⊥-module is extracted *syntactically* (no reasoning) and is both
/// justification- and entailment-preserving for the seed signature, so the
/// entailment check runs on the small module — never on full wine/sio. If the
/// module unexpectedly fails to entail (a locality-classifier bug dropping a
/// needed axiom), we re-check the full set and fall back to it: this keeps
/// find-one sound and find-all complete even if `is_bot_local` is wrong, paying
/// the full-ontology cost only on that (rare) negative.
///
/// The top-level verdict (#396): the full set decides whenever the module does
/// not entail, so a module give-up followed by a definite full-set "no" is a
/// definite `NotEntailed`, and a full-set give-up is `Unknown` even after a
/// definite module "no" (the module is only trusted to entail).
fn localized_candidates<A: ForIRI>(
    fixed: &[Component<A>],
    all_candidates: &[Component<A>],
    q: &Entailment,
) -> Result<Result<Vec<Component<A>>, Verdict>, ReasonError> {
    let full = || -> Result<Result<Vec<Component<A>>, Verdict>, ReasonError> {
        Ok(
            match entails_reporting(&ontology_from(fixed, all_candidates), q)? {
                Verdict::Entailed => Ok(canonical_order(all_candidates.to_vec())),
                v => Err(v),
            },
        )
    };
    // Escape hatch / differential-test gate: skip module extraction entirely.
    let no_module = std::env::var_os("RUSTDL_JUSTIFY_NO_MODULE").is_some();
    let Some(seed) = (if no_module {
        None
    } else {
        query_seed_signature(q)
    }) else {
        // Not localizable (or disabled): decide on the full set.
        return full();
    };
    let module = extract_bot_module(all_candidates, &seed);
    if std::env::var_os("RUSTDL_JUSTIFY_DEBUG").is_some() {
        eprintln!(
            "# justify ⊥-module: {} of {} logical axioms",
            module.len(),
            all_candidates.len()
        );
    }
    if entails_reporting(&ontology_from(fixed, &module), q)? == Verdict::Entailed {
        Ok(Ok(canonical_order(module)))
    } else {
        // Entailed here is a locality bug — safe fallback; otherwise genuinely not
        // entailed, or a give-up.
        full()
    }
}

/// Put the search input in a canonical, content-derived order.
///
/// [`SetOntology`] is backed by a `HashSet`, and std's `RandomState` is seeded
/// per PROCESS — so [`logical_axioms`] hands back the ontology's axioms in a
/// different order on every run. `QuickXplain` and the HST both follow their
/// input order, so without this the same query reports the same justification's
/// axioms in a different order each run, and where several minimal
/// justifications exist it can return a *different* (equally valid) one, or a
/// different subset of them under a `max` cap. Sorting the (usually small)
/// ⊥-module makes justify/justify-all/repair/report byte-reproducible.
fn canonical_order<A: ForIRI>(mut axioms: Vec<Component<A>>) -> Vec<Component<A>> {
    axioms.sort();
    axioms
}

/// A minimal (on EL/Horn) responsible-axiom set for an entailment.
#[derive(Debug, Clone)]
pub struct Justification<A: ForIRI> {
    pub axioms: Vec<Component<A>>,
    pub fragment: FragmentClassification,
    /// `true` iff minimality is guaranteed: the fragment is EL/Horn (rustdl
    /// complete) AND no subset probe of this justification's search gave up
    /// (#396). Otherwise the set still entails, but may not be minimal.
    pub minimal_guaranteed: bool,
}

/// The outcome of a justification query with its give-up signal (#396).
#[derive(Debug, Clone)]
pub struct JustifyOutcome<A: ForIRI> {
    /// Whether the query is entailed. `Unknown`: the top-level check gave up, so
    /// there are no justifications and no non-entailment either.
    pub status: Verdict,
    /// The justifications found (empty unless `status` is `Entailed`).
    pub justifications: Vec<Justification<A>>,
    /// `true` iff any probe gave up: the top-level one (`status` is `Unknown`), a
    /// `QuickXplain` subset probe (that justification's `minimal_guaranteed` is
    /// cleared), or an enumeration branch probe (a justification may be missing).
    /// Every justification returned still entails the query.
    pub incomplete: bool,
}

impl<A: ForIRI> JustifyOutcome<A> {
    /// The first justification, or `None` (not entailed, or unknown).
    fn into_first(self) -> Option<Justification<A>> {
        self.justifications.into_iter().next()
    }
}

/// Records whether any probe below the top-level one gave up (#396). Such a
/// probe answers in its sound direction ("not entailed"), so the search stays
/// sound, but a justification may be non-minimal or missing.
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct GiveUps {
    pub(crate) gave_up: bool,
}

impl GiveUps {
    /// `entails`, recording a give-up.
    pub(crate) fn entails<A: ForIRI>(
        &mut self,
        onto: &SetOntology<A>,
        q: &Entailment,
    ) -> Result<bool, ReasonError> {
        match entails_reporting(onto, q)? {
            Verdict::Entailed => Ok(true),
            Verdict::NotEntailed => Ok(false),
            Verdict::Unknown => {
                self.gave_up = true;
                Ok(false)
            }
        }
    }
}

/// A search's raw result: the top-level verdict, the justifications (each with
/// whether its own `QuickXplain` gave up), and whether any probe gave up.
struct Search<A: ForIRI> {
    status: Verdict,
    found: Vec<(Vec<Component<A>>, bool)>,
    gave_up: bool,
}

impl<A: ForIRI> Search<A> {
    fn top_level(status: Verdict) -> Self {
        Self {
            status,
            found: Vec::new(),
            gave_up: false,
        }
    }

    fn into_outcome(self, fragment: impl FnOnce() -> FragmentClassification) -> JustifyOutcome<A> {
        // Classify the fragment only once there is a justification to label: on the
        // not-entailed path that convert+analyze would be pure waste.
        let justifications = if self.found.is_empty() {
            Vec::new()
        } else {
            let fragment = fragment();
            self.found
                .into_iter()
                .map(|(axioms, gave_up)| justification(axioms, fragment, gave_up))
                .collect()
        };
        JustifyOutcome {
            status: self.status,
            justifications,
            incomplete: self.status == Verdict::Unknown || self.gave_up,
        }
    }
}

/// Per-ontology state precomputed once and reused across justification queries.
///
/// [`find_one_justification`]/[`find_all_justifications`] redo the full-ontology
/// work — the logical-axiom split and the fragment `convert`+classification — on
/// every call. Preparing once with [`PreparedJustifier::prepare`] and then
/// calling [`PreparedJustifier::find_one`]/[`PreparedJustifier::find_all`] per
/// query amortizes that across many justifications of the same ontology.
pub struct PreparedJustifier<A: ForIRI> {
    fixed: Vec<Component<A>>,
    all_candidates: Vec<Component<A>>,
    fragment: FragmentClassification,
}

impl<A: ForIRI> PreparedJustifier<A> {
    /// Precompute the reusable per-ontology state (logical-axiom split + fragment).
    #[must_use]
    pub fn prepare(onto: &SetOntology<A>) -> Self {
        let (fixed, all_candidates) = logical_axioms(onto);
        let fragment = fragment_of(onto);
        Self {
            fixed,
            all_candidates,
            fragment,
        }
    }

    /// The non-logical background (declarations, imports, ontology metadata) —
    /// never part of a justification, always part of the reasoned-over ontology.
    pub(crate) fn background(&self) -> &[Component<A>] {
        &self.fixed
    }

    /// Every logical axiom: the candidate pool a justification is drawn from.
    pub(crate) fn logical(&self) -> &[Component<A>] {
        &self.all_candidates
    }

    /// One minimal justification for `q` (`None` if not entailed, or if a probe
    /// gave up before deciding — use [`Self::find_one_reporting`] to tell those
    /// apart), reusing the prepared state.
    ///
    /// # Errors
    /// Propagates [`ReasonError`].
    pub fn find_one(&self, q: &Entailment) -> Result<Option<Justification<A>>, ReasonError> {
        Ok(self.find_one_reporting(q)?.into_first())
    }

    /// [`Self::find_one`] with the give-up signal (#396).
    ///
    /// # Errors
    /// Propagates [`ReasonError`].
    pub fn find_one_reporting(&self, q: &Entailment) -> Result<JustifyOutcome<A>, ReasonError> {
        Ok(search_one(&self.fixed, &self.all_candidates, q)?.into_outcome(|| self.fragment))
    }

    /// Up to `max` minimal justifications for `q`, reusing the prepared state.
    /// Empty if not entailed, or if a probe gave up before deciding — use
    /// [`Self::find_all_reporting`] to tell those apart.
    ///
    /// # Errors
    /// Propagates [`ReasonError`].
    pub fn find_all(
        &self,
        q: &Entailment,
        max: usize,
    ) -> Result<Vec<Justification<A>>, ReasonError> {
        Ok(self.find_all_reporting(q, max)?.justifications)
    }

    /// [`Self::find_all`] with the give-up signal (#396).
    ///
    /// # Errors
    /// Propagates [`ReasonError`].
    pub fn find_all_reporting(
        &self,
        q: &Entailment,
        max: usize,
    ) -> Result<JustifyOutcome<A>, ReasonError> {
        Ok(search_all(&self.fixed, &self.all_candidates, q, max)?.into_outcome(|| self.fragment))
    }
}

fn justification<A: ForIRI>(
    axioms: Vec<Component<A>>,
    fragment: FragmentClassification,
    gave_up: bool,
) -> Justification<A> {
    Justification {
        axioms,
        fragment,
        minimal_guaranteed: !gave_up
            && matches!(
                fragment,
                FragmentClassification::PureEl | FragmentClassification::Horn
            ),
    }
}

/// The `QuickXplain` search over an already-split ontology: axioms only, no
/// fragment classification, so callers that don't have one yet can stay lazy.
fn search_one<A: ForIRI>(
    fixed: &[Component<A>],
    all_candidates: &[Component<A>],
    q: &Entailment,
) -> Result<Search<A>, ReasonError> {
    let _sat_guard = maybe_saturation_oracle(fixed, all_candidates, q);
    let candidates = match localized_candidates(fixed, all_candidates, q)? {
        Ok(candidates) => candidates,
        // Not entailed (or unknown) — nothing to justify.
        Err(status) => return Ok(Search::top_level(status)),
    };
    let mut give_ups = GiveUps::default();
    let j = quickxplain(fixed, &candidates, q, &mut give_ups)?;
    Ok(Search {
        status: Verdict::Entailed,
        found: vec![(j, give_ups.gave_up)],
        gave_up: give_ups.gave_up,
    })
}

/// The HST search over an already-split ontology; see [`search_one`].
fn search_all<A: ForIRI>(
    fixed: &[Component<A>],
    all_candidates: &[Component<A>],
    q: &Entailment,
    max: usize,
) -> Result<Search<A>, ReasonError> {
    let _sat_guard = maybe_saturation_oracle(fixed, all_candidates, q);
    // Narrow to the query's ⊥-module up front: justification-preserving, so the
    // HST below still discovers *every* justification — over a far smaller set.
    let candidates = match localized_candidates(fixed, all_candidates, q)? {
        Ok(candidates) => candidates,
        Err(status) => return Ok(Search::top_level(status)), // not entailed (or unknown)
    };
    // A branch probe that gives up prunes its branch, so a justification may be
    // missing; that is recorded in `branch_give_ups`, separately from each
    // justification's own `QuickXplain`.
    let mut branch_give_ups = GiveUps::default();
    let mut found: Vec<(Vec<Component<A>>, bool)> = Vec::new();
    let mut seen: std::collections::HashSet<BTreeSet<Component<A>>> =
        std::collections::HashSet::new();
    // HST worklist: each node is a set of candidate-INDICES removed on the path.
    let mut worklist: Vec<BTreeSet<usize>> = vec![BTreeSet::new()];
    let mut explored: BTreeSet<BTreeSet<usize>> = BTreeSet::new();
    while let Some(removed) = worklist.pop() {
        if found.len() >= max {
            break;
        }
        if !explored.insert(removed.clone()) {
            continue;
        }
        let subset: Vec<Component<A>> = candidates
            .iter()
            .enumerate()
            .filter(|(i, _)| !removed.contains(i))
            .map(|(_, c)| c.clone())
            .collect();
        if !branch_give_ups.entails(&ontology_from(fixed, &subset), q)? {
            continue; // this branch cannot yield a justification
        }
        let mut give_ups = GiveUps::default();
        let j = quickxplain(fixed, &subset, q, &mut give_ups)?;
        branch_give_ups.gave_up |= give_ups.gave_up;
        // Record if this justification (as an axiom SET) is new.
        let key: BTreeSet<Component<A>> = j.iter().cloned().collect();
        if seen.insert(key) {
            found.push((j.clone(), give_ups.gave_up));
        }
        // Branch: remove each justification axiom (by its candidate index).
        for c in &j {
            if let Some(idx) = candidates.iter().position(|x| x == c) {
                let mut next = removed.clone();
                next.insert(idx);
                worklist.push(next);
            }
        }
    }
    Ok(Search {
        status: Verdict::Entailed,
        found,
        gave_up: branch_give_ups.gave_up,
    })
}

/// Find ONE justification for `q` in `onto`, or `Ok(None)` if `onto` does not
/// entail `q`. `QuickXplain` over the logical axioms; minimal on EL/Horn
/// (rustdl complete), guaranteed-entailing on SROIQ.
///
/// # Errors
/// Propagates [`ReasonError`].
///
/// A probe that gives up before deciding also yields `Ok(None)`; use
/// [`find_one_justification_reporting`] to tell that apart from a non-entailment.
pub fn find_one_justification<A: ForIRI>(
    onto: &SetOntology<A>,
    q: &Entailment,
) -> Result<Option<Justification<A>>, ReasonError> {
    Ok(find_one_justification_reporting(onto, q)?.into_first())
}

/// [`find_one_justification`] with the give-up signal (#396).
///
/// # Errors
/// Propagates [`ReasonError`].
pub fn find_one_justification_reporting<A: ForIRI>(
    onto: &SetOntology<A>,
    q: &Entailment,
) -> Result<JustifyOutcome<A>, ReasonError> {
    let (fixed, all_candidates) = logical_axioms(onto);
    // The fragment is classified only if there is a justification to label. (A
    // [`PreparedJustifier`] amortizes it instead — a single query cannot.)
    Ok(search_one(&fixed, &all_candidates, q)?.into_outcome(|| fragment_of(onto)))
}

fn fragment_of<A: ForIRI>(onto: &SetOntology<A>) -> FragmentClassification {
    owl_dl_core::convert::convert_ontology(onto)
        .map_or(FragmentClassification::OutOfFragment, |internal| {
            analyze_fragment(&internal)
        })
}

/// `QuickXplain` (Junker 2004): minimal `C' ⊆ candidates` with
/// `fixed ∪ C' ⊨ q`. Precondition: `fixed ∪ candidates ⊨ q`.
///
/// A subset probe that gives up reads as "not entailed", which only keeps more
/// axioms: the result still entails, but may not be minimal. Such a give-up is
/// recorded in `give_ups` (#396).
pub(crate) fn quickxplain<A: ForIRI>(
    fixed: &[Component<A>],
    candidates: &[Component<A>],
    q: &Entailment,
    give_ups: &mut GiveUps,
) -> Result<Vec<Component<A>>, ReasonError> {
    if give_ups.entails(&ontology_from(fixed, &[]), q)? {
        return Ok(Vec::new()); // background alone entails ⇒ no candidate needed
    }
    if candidates.len() <= 1 {
        return Ok(candidates.to_vec());
    }
    qx(fixed, true, candidates, q, give_ups)
}

/// Find up to `max` minimal justifications for `q` via a Reiter Hitting-Set
/// Tree over [`quickxplain`] (`QuickXplain`). Returns `[]` if `q` is not entailed.
///
/// When capped, the returned justifications are the first `max` found in the
/// HST's DFS traversal order — no particular preference (e.g. smallest) among
/// them is guaranteed.
///
/// A probe that gives up before deciding also yields `[]`; use
/// [`find_all_justifications_reporting`] to tell that apart from a
/// non-entailment, and to learn whether the enumeration lost a branch.
///
/// # Errors
/// Propagates [`ReasonError`].
pub fn find_all_justifications<A: ForIRI>(
    onto: &SetOntology<A>,
    q: &Entailment,
    max: usize,
) -> Result<Vec<Justification<A>>, ReasonError> {
    Ok(find_all_justifications_reporting(onto, q, max)?.justifications)
}

/// [`find_all_justifications`] with the give-up signal (#396).
///
/// # Errors
/// Propagates [`ReasonError`].
pub fn find_all_justifications_reporting<A: ForIRI>(
    onto: &SetOntology<A>,
    q: &Entailment,
    max: usize,
) -> Result<JustifyOutcome<A>, ReasonError> {
    let (fixed, all_candidates) = logical_axioms(onto);
    Ok(search_all(&fixed, &all_candidates, q, max)?.into_outcome(|| fragment_of(onto)))
}

/// `delta_nonempty`: whether the most recent addition to `fixed` was non-empty
/// (if empty, skip the redundant entailment check at this node).
fn qx<A: ForIRI>(
    fixed: &[Component<A>],
    delta_nonempty: bool,
    candidates: &[Component<A>],
    q: &Entailment,
    give_ups: &mut GiveUps,
) -> Result<Vec<Component<A>>, ReasonError> {
    if delta_nonempty && give_ups.entails(&ontology_from(fixed, &[]), q)? {
        return Ok(Vec::new());
    }
    if candidates.len() == 1 {
        return Ok(candidates.to_vec());
    }
    let mid = candidates.len() / 2;
    let (c1, c2) = candidates.split_at(mid);
    let fixed_c1: Vec<Component<A>> = fixed.iter().chain(c1.iter()).cloned().collect();
    let d2 = qx(&fixed_c1, !c1.is_empty(), c2, q, give_ups)?;
    let fixed_d2: Vec<Component<A>> = fixed.iter().chain(d2.iter()).cloned().collect();
    let d1 = qx(&fixed_d2, !d2.is_empty(), c1, q, give_ups)?;
    let mut out = d1;
    out.extend(d2);
    Ok(out)
}

#[cfg(test)]
mod verdict_tests {
    use super::{Verdict, guarded};
    use Verdict::{Entailed, NotEntailed, Unknown};

    const ALL: [Verdict; 3] = [Entailed, NotEntailed, Unknown];

    #[test]
    fn a_give_up_is_only_a_negative_called_into_question() {
        assert_eq!(Verdict::from_check(true, false), Entailed);
        assert_eq!(Verdict::from_check(true, true), Entailed);
        assert_eq!(Verdict::from_check(false, false), NotEntailed);
        assert_eq!(Verdict::from_check(false, true), Unknown);
    }

    #[test]
    fn and_is_decided_by_a_definite_no_on_either_side() {
        for a in ALL {
            for b in ALL {
                let mut ran = false;
                let got = a
                    .and(|| {
                        ran = true;
                        Ok(b)
                    })
                    .expect("the closure returns Ok");
                let want = match (a, b) {
                    (NotEntailed, _) | (_, NotEntailed) => NotEntailed,
                    (Entailed, Entailed) => Entailed,
                    _ => Unknown,
                };
                assert_eq!(got, want, "{a:?} AND {b:?}");
                // The second half runs unless the first is a definite no: after a
                // give-up it may still decide.
                assert_eq!(ran, a != NotEntailed, "{a:?} AND {b:?}");
            }
        }
    }

    // #396: a data-property guard that gave up ruled out nothing, so the main
    // probe's "inconsistent" must not become an entailment. On 293dcfd1 the guard
    // read the give-up as "consistent" and the probe reported
    // `SubDataPropertyOf(sub, sup)` entailed where HermiT says it is not.
    #[test]
    fn a_guard_give_up_never_yields_an_entailment() {
        for main in ALL {
            assert_eq!(guarded(false, main), main, "guard decided");
            assert_ne!(
                guarded(true, main),
                Entailed,
                "guard gave up, main {main:?}"
            );
        }
        assert_eq!(guarded(true, Entailed), Unknown);
        // A definite "no" from the main probe still decides: it only adds axioms
        // to the guard's ontology.
        assert_eq!(guarded(true, NotEntailed), NotEntailed);
    }
}
