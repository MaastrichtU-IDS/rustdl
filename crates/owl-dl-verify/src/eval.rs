//! Engine-blind axiom and concept evaluation.
//!
//! Generic over `Interpretation`, resolving concepts only through `ConceptPool`
//! — DATA, not saturation logic. An evaluator sharing code with the saturator
//! could hide the very bug this crate exists to find.
//!
//! NO WILDCARD MATCH ARM over `ConceptExpr` or `Axiom`. An unhandled form is
//! `Unresolved`, never a skip: otherwise "accept" can mean "ignored every form
//! I did not recognise".

use owl_dl_core::{Axiom, ConceptExpr as CE, ConceptId, ConceptPool, Role};

use crate::UnresolvedReason;
use crate::interp::{Element, Interpretation};

/// The three-valued outcome of judging a concept membership.
///
/// `Unresolved` is not an error path — it is the honest answer for a concept
/// form this phase cannot judge, and it must never collapse into `True` or
/// `False`: doing so would let an unhandled construct silently pass (or fail)
/// a check it was never actually evaluated against.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Judgement {
    True,
    False,
    Unresolved(&'static str),
}

/// Is element `e` a member of concept `c` under `interp`?
///
/// All 12 `ConceptExpr` variants are listed explicitly, so that adding a 13th
/// variant to the core enum breaks the build rather than silently becoming a
/// skip.
pub fn eval_concept<I: Interpretation>(
    pool: &ConceptPool,
    interp: &I,
    e: Element,
    c: ConceptId,
) -> Judgement {
    match pool.get(c) {
        CE::Top => Judgement::True,
        CE::Bot => Judgement::False,
        CE::Atomic(cls) => {
            if interp.in_concept(e, *cls) {
                Judgement::True
            } else {
                Judgement::False
            }
        }
        CE::And(ops) => {
            // A `False` operand short-circuits to `False` regardless of what
            // the other, possibly-unjudgeable, operands would say. Only if no
            // operand is `False` does an `Unresolved` operand propagate — the
            // conjunction can't be called `True` when part of it wasn't
            // actually judged.
            let mut unresolved = None;
            for op in ops {
                match eval_concept(pool, interp, e, *op) {
                    Judgement::False => return Judgement::False,
                    Judgement::Unresolved(v) => unresolved = Some(v),
                    Judgement::True => {}
                }
            }
            unresolved.map_or(Judgement::True, Judgement::Unresolved)
        }
        CE::Some(Role::Named(r), body) => {
            // A `True` witness short-circuits to `True` — one satisfying
            // successor is enough for an existential. Otherwise, an
            // `Unresolved` body on any successor makes the whole restriction
            // `Unresolved`, not `False`: absence of a confirmed witness is
            // not the same as a confirmed absence of one.
            let mut unresolved = None;
            for t in interp.successors(e, *r) {
                match eval_concept(pool, interp, t, *body) {
                    Judgement::True => return Judgement::True,
                    Judgement::Unresolved(v) => unresolved = Some(v),
                    Judgement::False => {}
                }
            }
            unresolved.map_or(Judgement::False, Judgement::Unresolved)
        }
        CE::Some(Role::Inverse(_), _) => Judgement::Unresolved("Some(Inverse)"),
        CE::Nominal(_) => Judgement::Unresolved("Nominal"),
        CE::SelfRestriction(_) => Judgement::Unresolved("SelfRestriction"),
        CE::Not(_) => Judgement::Unresolved("Not"),
        CE::Or(_) => Judgement::Unresolved("Or"),
        CE::All(_, _) => Judgement::Unresolved("All"),
        CE::Min(_, _, _) => Judgement::Unresolved("Min"),
        CE::Max(_, _, _) => Judgement::Unresolved("Max"),
    }
}

/// The outcome of checking one axiom against a model.
///
/// `Fails` carries the offending element(s) (a single element for a
/// concept-level check, an edge endpoint for `Domain`/`Range`) and a
/// human-readable note. Never renders a class/role IRI: `Vocabulary` is not
/// reachable from this module at all (by design — see the module doc), so
/// there is no `class_iri` to panic on a Tseitin id in the first place, and a
/// caller with vocabulary access should render witnesses by looking up the
/// `Element`'s label itself, never by treating a synthetic id as if it had
/// one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AxiomVerdict {
    Holds,
    Fails { witness: Vec<Element>, note: String },
    Unresolved(UnresolvedReason),
}

/// Three-valued outcome of one universally-quantified sub-check — one
/// element, or one edge endpoint. Shared by every checked axiom arm below so
/// they cannot each invent their own (and possibly diverging) three-valued
/// combining logic.
#[derive(Clone, Copy)]
enum LocalOutcome {
    Hold,
    Fail,
    Ambiguous(&'static str),
}

/// `ante → cons`, three-valued, at one element.
///
/// `cons == True` or `ante == False` decide the implication regardless of
/// the OTHER side — an `Unresolved` operand there cannot flip the verdict.
/// Short of that, an `Unresolved` operand on either remaining side makes the
/// implication undecidable (it might turn out `True` and satisfy the
/// implication, or not), and the sole remaining combination — `ante == True`,
/// `cons == False` — is the one genuine violation.
fn implication(ante: Judgement, cons: Judgement) -> LocalOutcome {
    if cons == Judgement::True {
        return LocalOutcome::Hold;
    }
    if ante == Judgement::False {
        return LocalOutcome::Hold;
    }
    if let Judgement::Unresolved(tag) = cons {
        return LocalOutcome::Ambiguous(tag);
    }
    if let Judgement::Unresolved(tag) = ante {
        return LocalOutcome::Ambiguous(tag);
    }
    LocalOutcome::Fail
}

/// Do every member of `js` agree (all `True` or all `False`) at one element?
///
/// A confirmed `True` alongside a confirmed `False` is a definite
/// disagreement regardless of any other `Unresolved` member. Short of that,
/// agreement is undecidable whenever an `Unresolved` member could plausibly
/// disagree with something: either there IS a determinate value it might
/// contradict, or at least two members are themselves `Unresolved` (either
/// could turn out to disagree with the other).
fn all_agree(js: &[Judgement]) -> LocalOutcome {
    let mut any_true = false;
    let mut any_false = false;
    let mut tag = None;
    let mut unresolved_count = 0usize;
    for j in js {
        match j {
            Judgement::True => any_true = true,
            Judgement::False => any_false = true,
            Judgement::Unresolved(v) => {
                tag = Some(*v);
                unresolved_count += 1;
            }
        }
    }
    if any_true && any_false {
        return LocalOutcome::Fail;
    }
    if let Some(v) = tag
        && (any_true || any_false || unresolved_count >= 2)
    {
        return LocalOutcome::Ambiguous(v);
    }
    LocalOutcome::Hold
}

/// Does at most one member of `js` hold at one element?
///
/// A second confirmed `True` is a definite violation regardless of any
/// `Unresolved` member. Short of that, the verdict is undecidable exactly
/// when the confirmed-`True` count plus the `Unresolved` count could still
/// reach two — an `Unresolved` member might resolve `True` and tip it over.
fn at_most_one(js: &[Judgement]) -> LocalOutcome {
    let mut true_count = 0usize;
    let mut unresolved_count = 0usize;
    let mut tag = None;
    for j in js {
        match j {
            Judgement::True => true_count += 1,
            Judgement::False => {}
            Judgement::Unresolved(v) => {
                tag = Some(*v);
                unresolved_count += 1;
            }
        }
    }
    if true_count >= 2 {
        return LocalOutcome::Fail;
    }
    if let Some(v) = tag
        && true_count + unresolved_count >= 2
    {
        return LocalOutcome::Ambiguous(v);
    }
    LocalOutcome::Hold
}

/// `Unresolved(UnhandledAxiom)` for a variant this evaluator never judges at
/// all — as opposed to `unresolved_or_holds`, which is for a variant that WAS
/// judged but hit an unhandled concept form partway through.
fn unhandled(axiom_index: usize, variant: &'static str) -> AxiomVerdict {
    AxiomVerdict::Unresolved(UnresolvedReason::UnhandledAxiom {
        axiom_index,
        variant,
    })
}

/// Folds a scan's ambiguity flag into a verdict: no ambiguity and no `Fail`
/// was seen (a `Fail` returns directly from the scan, never reaching here)
/// means every element checked out, so `Holds`; an ambiguity means some
/// element's verdict genuinely depended on a concept form this evaluator
/// cannot judge.
fn unresolved_or_holds(axiom_index: usize, ambiguous: Option<&'static str>) -> AxiomVerdict {
    match ambiguous {
        Some(variant) => AxiomVerdict::Unresolved(UnresolvedReason::UnhandledConcept {
            axiom_index,
            variant,
        }),
        None => AxiomVerdict::Holds,
    }
}

/// Is axiom `ax` (at `index` in its ontology's axiom list) satisfied by
/// `interp`?
///
/// All 25 `Axiom` variants are listed explicitly (spec §8's count, verified
/// against `crates/owl-dl-core/src/ontology.rs`), so that a 26th variant
/// breaks the build rather than silently becoming a skip. This task covers 8
/// of them (3 vacuous declarations, `SubClassOf`, `EquivalentClasses`,
/// `DisjointClasses`, `ObjectPropertyDomain`, `ObjectPropertyRange`); a later
/// task upgrades 5 more (the role-shaped variants) from the placeholder
/// `unhandled` arm to a real check. The remaining 12 have no evaluator planned
/// at all and stay `unhandled` permanently.
pub fn check_axiom<I: Interpretation>(
    pool: &ConceptPool,
    interp: &I,
    index: usize,
    ax: &Axiom,
) -> AxiomVerdict {
    match ax {
        Axiom::DeclareClass(_)
        | Axiom::DeclareObjectProperty(_)
        | Axiom::DeclareNamedIndividual(_) => AxiomVerdict::Holds,

        Axiom::SubClassOf { sub, sup } => {
            let mut ambiguous = None;
            for e in interp.elements() {
                let ante = eval_concept(pool, interp, e, *sub);
                let cons = eval_concept(pool, interp, e, *sup);
                match implication(ante, cons) {
                    LocalOutcome::Fail => {
                        return AxiomVerdict::Fails {
                            witness: vec![e],
                            note: format!(
                                "SubClassOf (axiom {index}): element {e:?} satisfies the \
                                 subclass but not the superclass"
                            ),
                        };
                    }
                    LocalOutcome::Ambiguous(tag) => ambiguous = Some(tag),
                    LocalOutcome::Hold => {}
                }
            }
            unresolved_or_holds(index, ambiguous)
        }

        Axiom::EquivalentClasses(members) => {
            let mut ambiguous = None;
            for e in interp.elements() {
                let js: Vec<Judgement> = members
                    .iter()
                    .map(|m| eval_concept(pool, interp, e, *m))
                    .collect();
                match all_agree(&js) {
                    LocalOutcome::Fail => {
                        return AxiomVerdict::Fails {
                            witness: vec![e],
                            note: format!(
                                "EquivalentClasses (axiom {index}): members disagree at \
                                 element {e:?}"
                            ),
                        };
                    }
                    LocalOutcome::Ambiguous(tag) => ambiguous = Some(tag),
                    LocalOutcome::Hold => {}
                }
            }
            unresolved_or_holds(index, ambiguous)
        }

        Axiom::DisjointClasses(members) => {
            let mut ambiguous = None;
            for e in interp.elements() {
                let js: Vec<Judgement> = members
                    .iter()
                    .map(|m| eval_concept(pool, interp, e, *m))
                    .collect();
                match at_most_one(&js) {
                    LocalOutcome::Fail => {
                        return AxiomVerdict::Fails {
                            witness: vec![e],
                            note: format!(
                                "DisjointClasses (axiom {index}): two or more members hold \
                                 at element {e:?}"
                            ),
                        };
                    }
                    LocalOutcome::Ambiguous(tag) => ambiguous = Some(tag),
                    LocalOutcome::Hold => {}
                }
            }
            unresolved_or_holds(index, ambiguous)
        }

        Axiom::ObjectPropertyDomain { role, domain } => {
            if role.is_inverse() {
                return AxiomVerdict::Unresolved(UnresolvedReason::UnhandledAxiom {
                    axiom_index: index,
                    variant: "ObjectPropertyDomain(Inverse)",
                });
            }
            let mut ambiguous = None;
            for (from, _to) in interp.edges(role.role_id()) {
                match eval_concept(pool, interp, from, *domain) {
                    Judgement::True => {}
                    Judgement::False => {
                        return AxiomVerdict::Fails {
                            witness: vec![from],
                            note: format!(
                                "ObjectPropertyDomain (axiom {index}): edge source {from:?} \
                                 is not in the domain"
                            ),
                        };
                    }
                    Judgement::Unresolved(tag) => ambiguous = Some(tag),
                }
            }
            unresolved_or_holds(index, ambiguous)
        }

        Axiom::ObjectPropertyRange { role, range } => {
            if role.is_inverse() {
                return AxiomVerdict::Unresolved(UnresolvedReason::UnhandledAxiom {
                    axiom_index: index,
                    variant: "ObjectPropertyRange(Inverse)",
                });
            }
            let mut ambiguous = None;
            for (_from, to) in interp.edges(role.role_id()) {
                match eval_concept(pool, interp, to, *range) {
                    Judgement::True => {}
                    Judgement::False => {
                        return AxiomVerdict::Fails {
                            witness: vec![to],
                            note: format!(
                                "ObjectPropertyRange (axiom {index}): edge target {to:?} is \
                                 not in the range"
                            ),
                        };
                    }
                    Judgement::Unresolved(tag) => ambiguous = Some(tag),
                }
            }
            unresolved_or_holds(index, ambiguous)
        }

        // The 5 role-shaped variants a later task upgrades to real checks.
        Axiom::SubObjectPropertyOf { .. } => unhandled(index, "SubObjectPropertyOf"),
        Axiom::EquivalentObjectProperties(_) => unhandled(index, "EquivalentObjectProperties"),
        Axiom::TransitiveRole(_) => unhandled(index, "TransitiveRole"),
        Axiom::SymmetricRole(_) => unhandled(index, "SymmetricRole"),
        Axiom::InverseObjectProperties(_, _) => unhandled(index, "InverseObjectProperties"),

        // The 12 variants with no evaluator planned at all.
        Axiom::DisjointUnion { .. } => unhandled(index, "DisjointUnion"),
        Axiom::DisjointObjectProperties(_) => unhandled(index, "DisjointObjectProperties"),
        Axiom::AsymmetricRole(_) => unhandled(index, "AsymmetricRole"),
        Axiom::ReflexiveRole(_) => unhandled(index, "ReflexiveRole"),
        Axiom::IrreflexiveRole(_) => unhandled(index, "IrreflexiveRole"),
        Axiom::FunctionalRole(_) => unhandled(index, "FunctionalRole"),
        Axiom::InverseFunctionalRole(_) => unhandled(index, "InverseFunctionalRole"),
        Axiom::ClassAssertion { .. } => unhandled(index, "ClassAssertion"),
        Axiom::ObjectPropertyAssertion { .. } => unhandled(index, "ObjectPropertyAssertion"),
        Axiom::NegativeObjectPropertyAssertion { .. } => {
            unhandled(index, "NegativeObjectPropertyAssertion")
        }
        Axiom::SameIndividual(_) => unhandled(index, "SameIndividual"),
        Axiom::DifferentIndividuals(_) => unhandled(index, "DifferentIndividuals"),
    }
}
