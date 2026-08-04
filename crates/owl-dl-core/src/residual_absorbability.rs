//! Residual-GCI **absorbability** census — report-only diagnostic.
//!
//! `absorb::as_trigger` recognises exactly two disjunct shapes,
//! `Not(Atomic)` and `Not(Nominal)`. A GCI carrying neither becomes a
//! [`AbsorbedTBox::residual_gcis`](crate::absorb::AbsorbedTBox) entry — a
//! **global disjunction re-applied at every node of the completion graph**.
//! `docs/2026-08-01-absorption-is-the-bottleneck.md` measures two extra
//! residual disjunctions costing 300× on `ore_ont_3281`.
//!
//! This module answers the *precondition* question for that work: across
//! the real population, **which absorption technique would remove which
//! residual?** It changes no reasoning behaviour — it only classifies.
//!
//! ## Soundness boundaries encoded here
//!
//! The classifier's whole value is that its `domain_absorbable` count is
//! not inflated by shapes that would be **unsound** to absorb as a domain
//! axiom. Concretely, a residual body is a disjunction `d₁ ⊔ … ⊔ dₙ`
//! standing for the GCI `⊤ ⊑ d₁ ⊔ … ⊔ dₙ`; if `dᵢ = ¬X` then the axiom is
//! `X ⊑ (rest)`. So the *antecedent* is the negation of the disjunct:
//!
//! | disjunct (NNF) | antecedent | domain axiom? |
//! |---|---|---|
//! | `Max(0, R, ⊤)` = `¬(≥1 R)` | `≥1 R` | **yes** — `ObjectPropertyDomain(R, rest)` |
//! | `All(R, ⊥)` = `¬∃R.⊤` | `∃R.⊤` | **yes** — same axiom |
//! | `Max(k, R, _)`, `k ≥ 1` | `≥k+1 R`, `k+1 ≥ 2` | **NO — unsound.** A domain rule fires at *one* successor; the antecedent needs `k+1`. Strictly too strong. |
//! | `All(R, D)`, `D ≠ ⊥` | `∃R.¬D` | **NO** — *qualified*; needs a filler check, does not reduce to a plain domain axiom. |
//! | `Max(0, R, C)`, `C ≠ ⊤` | `∃R.C` | **NO** — same qualified case, written as a `≤0` cardinality. |
//!
//! The last three get their own buckets ([`Bucket::CardAntecedentNGt1`],
//! [`Bucket::QualifiedExistsAntecedent`]) precisely so they cannot silently
//! swell the domain count and send an implementation at an unsound fix.

use crate::absorb::AbsorbedTBox;
use crate::ir::{ClassId, ConceptExpr, ConceptId, ConceptPool};
use crate::vocab::Vocabulary;
use std::collections::HashMap;

/// Every class IRI rustdl mints itself lives under this reserved namespace
/// (`urn:rustdl-dkey:` for concrete-domain keys, `urn:rustdl-anon:` for
/// anonymous individuals, `urn:rustdl-aux-role:` for chain aux roles). A
/// class whose IRI starts with it is **synthetic**, i.e. not written in the
/// source ontology.
pub const SYNTHETIC_CLASS_IRI_PREFIX: &str = "urn:rustdl-";

/// Is this class one rustdl minted, rather than one the ontology declares?
#[must_use]
pub fn is_synthetic_class_iri(iri: &str) -> bool {
    iri.starts_with(SYNTHETIC_CLASS_IRI_PREFIX)
}

/// Which qualifying disjunct shapes an `Or` conclusion carries — the raw
/// material for [`ResidualAbsorbabilityStats::concept_rule_or_guard_manufacturable`].
///
/// ## The mechanism being counted
///
/// `docs/2026-08-04-konclude-cardinality-mechanism.md` measures Konclude's
/// absorbed `TBox`: **all 47** of its absorbed rules carry **two** guards and
/// **zero** fire on a bare node, where rustdl's carry one and 10 fire on
/// every bare `CarbonAtom`. Konclude does not *find* its second guard among
/// the definition's atomic conjuncts — it **manufactures** one. For an
/// `∃r.F` conjunct of the body it mints a fresh marker `T` and emits
/// `F ⊑ ∀r⁻.T`, so `T` reaches exactly the nodes with an `r`-successor in
/// `F`, and the absorbed rule takes `T` as its second guard. The `≤n`
/// halves stay negated in the head.
///
/// So the question this shape answers is: **did the definition body have a
/// conjunct entailing `∃r.F` for some usable filler `F`?** Post-NNF, in the
/// absorbed `Or` conclusion, that conjunct appears negated:
///
/// | body conjunct | negated, NNF | tier |
/// |---|---|---|
/// | `∃r.F`, `F` a named class | `All(r, Not(Atomic F))` | **A** ([`Self::named`]) |
/// | `≥1 r.F` (incl. the `≥` half of `=1 r.F`), `F` named | `Max(0, r, F)` | **A** |
/// | `≥k r.F`, `k ≥ 2`, `F` named | `Max(k-1, r, F)`, `k-1 ≥ 1` | **B** ([`Self::card_ge2`]) |
/// | `∃r.F` / `≥k r.F` with `F` **complex** (e.g. `A ⊓ ∃s.B`) | `All(r, Or(…))` etc. | **C** ([`Self::complex`]) |
/// | `≤n r.F` (incl. the `≤` half of `=n r.F`) | `Min(n+1, r, F)` | **never** |
///
/// **The `Min` exclusion is the whole point of the predicate.** `≤n r.F`
/// does not entail `∃r.F` — it is satisfied by a node with no `r`-successor
/// at all — so no marker can be propagated to a node from it and no guard
/// can be manufactured. A predicate that counted every cardinality disjunct
/// would report an addressable population that is too optimistic.
///
/// ### Why three tiers and not one number
///
/// The tiers differ in *what rustdl would have to build*, so collapsing them
/// hides the cost:
///
/// - **A** needs only `F ⊑ ∀r⁻.T` for atomic `F` — a **single-trigger**
///   `ConceptRule`, a shape `absorb.rs` already emits. The only new capability
///   is a multi-guard `ConceptRule` at the *consuming* end.
/// - **B** is A plus recognising that `¬(≥k r.F)` also witnesses `∃r.F`.
/// - **C** needs `F ⊑ ∀r⁻.T` for a **complex** `F` (`SulfurAtom ⊓ ∃hDBW.O ⊓
///   ≥2 hDBW.O` on `ore_ont_10019`), i.e. the guard-minting must **recurse**
///   into the filler and compose markers — exactly the
///   `TRIG283 ⊓ TRIG329 → TRIG331` chain measured in Konclude's absorbed
///   `TBox`. Manufacturable, but it presupposes the multi-guard machinery it is
///   feeding.
///
/// `All(r, ⊥)` (an unqualified `∃r.⊤` conjunct) and `Max(k, r, ⊤)` are
/// **not** counted in any tier: `⊤ ⊑ ∀r⁻.T` marks every node with an
/// `r`-predecessor, which is not a class guard at all.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "four independent, orthogonal tier flags; a state machine would \
              lose that a rule can qualify in several tiers at once"
)]
pub struct GuardManufacturability {
    /// **Tier A — the headline.** A qualifying disjunct (`All(r, ¬F)` or
    /// `Max(0, r, F)`) whose filler `F` is a **named atomic** class.
    pub named: bool,
    /// **Tier B.** A `Max(k, r, F)` disjunct with `k ≥ 1` and `F` a named
    /// atomic class — the negation of a `≥(k+1) r.F` body conjunct, which
    /// *does* entail `∃r.F`.
    pub card_ge2: bool,
    /// **Tier C.** A qualifying disjunct whose filler is a **complex**
    /// (non-atomic, non-`⊤`, non-`⊥`) concept.
    pub complex: bool,
    /// A qualifying disjunct whose atomic filler is **synthetic**
    /// (`urn:rustdl-*`, e.g. `DKey`). Reported apart so a DKey-heavy ontology
    /// cannot inflate the named count.
    pub synthetic: bool,
}

/// What kind of filler a qualifying disjunct carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Filler {
    /// A named source-ontology class.
    NamedAtomic,
    /// A rustdl-minted class (`urn:rustdl-*`).
    SyntheticAtomic,
    /// Anything else that is neither `⊤` nor `⊥`.
    Complex,
    /// `⊤` or `⊥` — no class guard obtainable.
    Unusable,
}

impl GuardManufacturability {
    /// Classify one absorbed-rule conclusion. Pure; report-only.
    ///
    /// `named` decides whether a filler [`ClassId`] is a source-ontology
    /// class. Callers with a [`Vocabulary`] should pass
    /// `|c| !is_synthetic_class_iri(vocab.class_iri(c))`.
    #[must_use]
    pub fn classify(
        conclusion: ConceptId,
        pool: &ConceptPool,
        named: &dyn Fn(ClassId) -> bool,
    ) -> Self {
        let disjuncts: &[ConceptId] = match pool.get(conclusion) {
            ConceptExpr::Or(args) => args,
            _ => return Self::default(),
        };
        let mut out = Self::default();
        for &d in disjuncts {
            // (filler kind, is this the `k ≥ 1` cardinality form?)
            let (filler, card_ge2) = match pool.get(d) {
                // `∀r.X` = `¬∃r.¬X`, so the body conjunct was `∃r.¬X`.
                ConceptExpr::All(_, inner) => {
                    (Self::filler_of_negation(*inner, pool, named), false)
                }
                // `≤k r.F` = `¬(≥(k+1) r.F)`; the body conjunct was
                // `≥(k+1) r.F`, which entails `∃r.F` for every k ≥ 0.
                ConceptExpr::Max(k, _, filler) => (Self::filler(*filler, pool, named), *k >= 1),
                // `Min(k, r, F)` NEVER qualifies — see the type docs.
                _ => (Filler::Unusable, false),
            };
            match (filler, card_ge2) {
                (Filler::NamedAtomic, false) => out.named = true,
                (Filler::NamedAtomic, true) => out.card_ge2 = true,
                (Filler::Complex, _) => out.complex = true,
                (Filler::SyntheticAtomic, _) => out.synthetic = true,
                (Filler::Unusable, _) => {}
            }
        }
        out
    }

    /// Classify `F` where the disjunct is `∀r.X` and `F = ¬X`.
    fn filler_of_negation(
        x: ConceptId,
        pool: &ConceptPool,
        named: &dyn Fn(ClassId) -> bool,
    ) -> Filler {
        match pool.get(x) {
            // `∀r.⊥` = `¬∃r.⊤` ⇒ filler ⊤; `∀r.⊤` is trivially true.
            ConceptExpr::Bot | ConceptExpr::Top => Filler::Unusable,
            ConceptExpr::Not(inner) => match pool.get(*inner) {
                ConceptExpr::Atomic(c) => {
                    if named(*c) {
                        Filler::NamedAtomic
                    } else {
                        Filler::SyntheticAtomic
                    }
                }
                _ => Filler::Complex,
            },
            _ => Filler::Complex,
        }
    }

    /// Classify a filler given directly (the cardinality forms).
    fn filler(f: ConceptId, pool: &ConceptPool, named: &dyn Fn(ClassId) -> bool) -> Filler {
        match pool.get(f) {
            ConceptExpr::Top | ConceptExpr::Bot => Filler::Unusable,
            ConceptExpr::Atomic(c) => {
                if named(*c) {
                    Filler::NamedAtomic
                } else {
                    Filler::SyntheticAtomic
                }
            }
            _ => Filler::Complex,
        }
    }
}

/// Which absorption technique (if any) would remove a given residual GCI.
///
/// Buckets are **mutually exclusive** — [`Bucket::classify`] assigns
/// exactly one, most-specific-first, in the declaration order below.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Bucket {
    /// Carries `Max(0, R, ⊤)` or `All(R, ⊥)`. The GCI is `(≥1 R) ⊑ rest`,
    /// **logically identical** to `ObjectPropertyDomain(R, rest)`: sound and
    /// complete to absorb as a domain rule.
    DomainAbsorbable,
    /// Two or more `Not(Atomic)` disjuncts — binary absorption
    /// (Hudek & Weddell) would fire only when all the atomic conjuncts are
    /// present, instead of leaving a disjunction on every `A`-node.
    ///
    /// Expected to be **empty among residuals**: a residual by definition
    /// has *no* `Not(Atomic)` disjunct (else `as_trigger` would have picked
    /// it). Counted anyway so the report states that rather than assumes it.
    /// The place binary absorption actually pays is
    /// [`ResidualAbsorbabilityStats::concept_rule_or_with_extra_not_atomic`].
    BinaryAbsorbable,
    /// Carries `Not(Nominal)`. Also expected empty among residuals, for the
    /// same reason; counted for completeness.
    NominalAbsorbable,
    /// Carries `Max(k, R, _)` with `k ≥ 1` (and nothing more specific).
    /// **Not** domain-absorbable — see the module table.
    CardAntecedentNGt1,
    /// Carries `All(R, D)` with `D ≠ ⊥`, or `Max(0, R, C)` with `C ≠ ⊤`
    /// (and nothing more specific): a *qualified* existential antecedent.
    /// **Not** domain-absorbable — needs a filler check.
    QualifiedExistsAntecedent,
    /// None of the above. The floor: the part no absorption technique
    /// listed here removes.
    GenuinelyDisjunctive,
}

impl Bucket {
    /// Short stable name, used for the CLI histogram and the census tables.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::DomainAbsorbable => "domain_absorbable",
            Self::BinaryAbsorbable => "binary_absorbable",
            Self::NominalAbsorbable => "nominal_absorbable",
            Self::CardAntecedentNGt1 => "card_antecedent_n_gt_1",
            Self::QualifiedExistsAntecedent => "qualified_exists_antecedent",
            Self::GenuinelyDisjunctive => "genuinely_disjunctive",
        }
    }

    /// Classify one residual GCI body. Pure — no graph access, no mutation.
    ///
    /// `body` is the internalised `⊤ ⊑ body` disjunction; a non-`Or` body is
    /// treated as the singleton disjunct set, matching
    /// [`crate::absorb`]'s own handling.
    #[must_use]
    pub fn classify(body: ConceptId, pool: &ConceptPool) -> Self {
        let disjuncts: &[ConceptId] = match pool.get(body) {
            ConceptExpr::Or(args) => args,
            _ => std::slice::from_ref(&body),
        };

        let mut has_domain = false;
        let mut has_card_n_gt_1 = false;
        let mut has_qualified = false;
        let mut not_atomic_count = 0usize;
        let mut has_not_nominal = false;

        for &d in disjuncts {
            match pool.get(d) {
                // `≤0 R.C` = `¬∃R.C`. Unqualified (`C = ⊤`) ⟹ domain axiom;
                // qualified (`C ≠ ⊤`) ⟹ needs a filler check.
                ConceptExpr::Max(0, _, filler) => {
                    if matches!(pool.get(*filler), ConceptExpr::Top) {
                        has_domain = true;
                    } else {
                        has_qualified = true;
                    }
                }
                // `≤k R.C` with k ≥ 1 = `¬(≥k+1 R.C)`. Antecedent needs
                // k+1 ≥ 2 successors — UNSOUND to absorb as a domain rule.
                ConceptExpr::Max(_, _, _) => has_card_n_gt_1 = true,
                // `∀R.⊥` = `¬∃R.⊤` ⟹ domain axiom.
                // `∀R.D` with D ≠ ⊥ = `¬∃R.¬D` ⟹ qualified antecedent.
                ConceptExpr::All(_, inner) => {
                    if matches!(pool.get(*inner), ConceptExpr::Bot) {
                        has_domain = true;
                    } else {
                        has_qualified = true;
                    }
                }
                ConceptExpr::Not(inner) => match pool.get(*inner) {
                    ConceptExpr::Atomic(_) => not_atomic_count += 1,
                    ConceptExpr::Nominal(_) => has_not_nominal = true,
                    _ => {}
                },
                _ => {}
            }
        }

        if has_domain {
            Self::DomainAbsorbable
        } else if not_atomic_count >= 2 {
            Self::BinaryAbsorbable
        } else if has_not_nominal {
            Self::NominalAbsorbable
        } else if has_card_n_gt_1 {
            Self::CardAntecedentNGt1
        } else if has_qualified {
            Self::QualifiedExistsAntecedent
        } else {
            Self::GenuinelyDisjunctive
        }
    }
}

/// Per-ontology histogram produced by [`census`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ResidualAbsorbabilityStats {
    /// Total residual GCIs (== the sum of the six buckets).
    pub residual_gcis: usize,
    pub domain_absorbable: usize,
    pub binary_absorbable: usize,
    pub nominal_absorbable: usize,
    pub card_antecedent_n_gt_1: usize,
    pub qualified_exists_antecedent: usize,
    pub genuinely_disjunctive: usize,
    /// Total absorbed concept rules (context for the two counts below).
    pub concept_rules: usize,
    /// Concept rules whose conclusion is an `Or` — i.e. `as_trigger`
    /// consumed one `Not(Atomic)` and left a disjunction behind.
    pub concept_rule_or: usize,
    /// Concept rules whose conclusion is an `Or` **still carrying at least
    /// one `Not(Atomic)` disjunct**. This is where binary absorption pays:
    /// `A ⊓ B ⊑ C` absorbs to `A → (¬B ⊔ C)`, a disjunction fired on every
    /// `A`-node, where binary absorption would fire only when both `A` and
    /// `B` are present.
    pub concept_rule_or_with_extra_not_atomic: usize,
    /// **The headline (tier A).** `Or`-conclusion rules for which a second
    /// guard is *manufacturable* in Konclude's sense from a body conjunct
    /// `∃r.F` / `≥1 r.F` with `F` a **named atomic** class, so the marker rule
    /// `F ⊑ ∀r⁻.T` is itself single-triggered. See
    /// [`GuardManufacturability`].
    ///
    /// Contrast [`Self::concept_rule_or_with_extra_not_atomic`], which
    /// requires the second guard to be *already written* as an atomic conjunct
    /// of the definition: on `ore_ont_10019` that reads **0** of 29 where this
    /// reads **15**, and the three tiers together read **26**.
    pub concept_rule_or_guard_manufacturable: usize,
    /// Tier B only: qualify via `Max(k ≥ 1, r, F)` (a `≥(k+1) r.F` body
    /// conjunct) and **not** via tier A.
    pub concept_rule_or_guard_manufacturable_card_ge2_only: usize,
    /// Tier C only: qualify only via a **complex** filler, so the marker rule
    /// `F ⊑ ∀r⁻.T` needs a multi-guard / recursive absorption of its own.
    pub concept_rule_or_guard_manufacturable_complex_only: usize,
    /// Rules that qualify **only** via a synthetic (`urn:rustdl-*`) atomic
    /// filler. Excluded from every tier.
    pub concept_rule_or_guard_manufacturable_synthetic_only: usize,
    /// Trigger classes that are the trigger of **≥2** `Or`-conclusion rules.
    /// This is the quantity that makes `ore_ont_10019` quadratic: a bare node
    /// labelled such a trigger acquires that many open disjunctions at once.
    pub distinct_shared_triggers: usize,
    /// The largest number of `Or`-conclusion rules sharing one trigger
    /// (`ore_ont_10019`: 10, on `CarbonAtom`).
    pub max_disjunctive_rules_per_trigger: usize,
    /// As [`Self::distinct_shared_triggers`], but with the threshold at ≥5.
    pub shared_triggers_ge5: usize,
}

impl ResidualAbsorbabilityStats {
    /// Residuals that domain absorption alone would eliminate.
    #[must_use]
    pub fn removed_by_domain(&self) -> usize {
        self.domain_absorbable
    }

    /// Residuals that domain + binary absorption together would eliminate.
    #[must_use]
    pub fn removed_by_domain_and_binary(&self) -> usize {
        self.domain_absorbable + self.binary_absorbable
    }

    /// `true` iff the ontology would be left with **zero** residual GCIs —
    /// i.e. no global disjunctions at all — under domain absorption alone.
    #[must_use]
    pub fn zero_residuals_under_domain(&self) -> bool {
        self.removed_by_domain() == self.residual_gcis
    }

    /// As [`Self::zero_residuals_under_domain`], for domain + binary.
    #[must_use]
    pub fn zero_residuals_under_domain_and_binary(&self) -> bool {
        self.removed_by_domain_and_binary() == self.residual_gcis
    }

    /// Tiers A + B + C — the Konclude-equivalent total (`ore_ont_10019`: 26).
    #[must_use]
    pub fn guard_manufacturable_any_tier(&self) -> usize {
        self.concept_rule_or_guard_manufacturable
            + self.concept_rule_or_guard_manufacturable_card_ge2_only
            + self.concept_rule_or_guard_manufacturable_complex_only
    }

    /// `true` iff **every** `Or`-conclusion rule is tier-A manufacturable —
    /// the strict reading of "N disjunctive rules firing on bare nodes → 0".
    /// Vacuously false when there are no `Or`-conclusion rules at all.
    #[must_use]
    pub fn all_or_rules_guard_manufacturable(&self) -> bool {
        self.concept_rule_or > 0
            && self.concept_rule_or_guard_manufacturable == self.concept_rule_or
    }

    /// As [`Self::all_or_rules_guard_manufacturable`], allowing any tier —
    /// i.e. what Konclude actually achieves (all 47 rules, 0 bare-node fires).
    #[must_use]
    pub fn all_or_rules_guard_manufacturable_any_tier(&self) -> bool {
        self.concept_rule_or > 0 && self.guard_manufacturable_any_tier() == self.concept_rule_or
    }

    fn bump(&mut self, b: Bucket) {
        match b {
            Bucket::DomainAbsorbable => self.domain_absorbable += 1,
            Bucket::BinaryAbsorbable => self.binary_absorbable += 1,
            Bucket::NominalAbsorbable => self.nominal_absorbable += 1,
            Bucket::CardAntecedentNGt1 => self.card_antecedent_n_gt_1 += 1,
            Bucket::QualifiedExistsAntecedent => self.qualified_exists_antecedent += 1,
            Bucket::GenuinelyDisjunctive => self.genuinely_disjunctive += 1,
        }
    }
}

/// Classify every residual GCI of `tbox` and summarise. Report-only.
///
/// `vocab` distinguishes source-ontology classes from rustdl-minted ones for
/// the guard-manufacturability counters. Passing `None` treats **every**
/// class as named — correct only for hand-built test fixtures that intern no
/// synthetic class; real callers should pass `Some(&internal.vocabulary)`.
#[must_use]
pub fn census(
    tbox: &AbsorbedTBox,
    pool: &ConceptPool,
    vocab: Option<&Vocabulary>,
) -> ResidualAbsorbabilityStats {
    let mut stats = ResidualAbsorbabilityStats {
        residual_gcis: tbox.residual_gcis.len(),
        concept_rules: tbox.concept_rules.len(),
        ..ResidualAbsorbabilityStats::default()
    };
    for &r in &tbox.residual_gcis {
        stats.bump(Bucket::classify(r, pool));
    }
    let named = |c: ClassId| match vocab {
        Some(v) => !is_synthetic_class_iri(v.class_iri(c)),
        None => true,
    };
    let mut per_trigger: HashMap<ClassId, usize> = HashMap::new();
    for rule in &tbox.concept_rules {
        if let ConceptExpr::Or(args) = pool.get(rule.conclusion) {
            stats.concept_rule_or += 1;
            *per_trigger.entry(rule.trigger).or_default() += 1;
            let extra = args.iter().any(|&d| {
                matches!(pool.get(d), ConceptExpr::Not(inner)
                    if matches!(pool.get(*inner), ConceptExpr::Atomic(_)))
            });
            if extra {
                stats.concept_rule_or_with_extra_not_atomic += 1;
            }
            let g = GuardManufacturability::classify(rule.conclusion, pool, &named);
            if g.named {
                stats.concept_rule_or_guard_manufacturable += 1;
            } else if g.card_ge2 {
                stats.concept_rule_or_guard_manufacturable_card_ge2_only += 1;
            } else if g.complex {
                stats.concept_rule_or_guard_manufacturable_complex_only += 1;
            } else if g.synthetic {
                stats.concept_rule_or_guard_manufacturable_synthetic_only += 1;
            }
        }
    }
    stats.distinct_shared_triggers = per_trigger.values().filter(|&&n| n >= 2).count();
    stats.shared_triggers_ge5 = per_trigger.values().filter(|&&n| n >= 5).count();
    stats.max_disjunctive_rules_per_trigger = per_trigger.values().copied().max().unwrap_or(0);
    stats
}

#[cfg(test)]
#[allow(clippy::many_single_char_names)]
mod tests {
    use super::*;
    use crate::absorb::ConceptRule;
    use crate::ir::{ClassId, IndividualId, Role, RoleId};

    fn r0() -> Role {
        Role::Named(RoleId::new(0))
    }

    /// `(≥1 R) ⊑ A` internalises to `Max(0,R,⊤) ⊔ A` — a domain axiom.
    #[test]
    fn max_zero_unqualified_is_domain_absorbable() {
        let mut pool = ConceptPool::new();
        let top = pool.top();
        let m = pool.max(0, r0(), top);
        let a = pool.atomic(ClassId::new(0));
        let body = pool.or([m, a]);
        assert_eq!(Bucket::classify(body, &pool), Bucket::DomainAbsorbable);
    }

    /// `∃R.⊤ ⊑ A` internalises to `∀R.⊥ ⊔ A` — the same domain axiom.
    #[test]
    fn all_bot_is_domain_absorbable() {
        let mut pool = ConceptPool::new();
        let bot = pool.bot();
        let all = pool.all(r0(), bot);
        let a = pool.atomic(ClassId::new(0));
        let body = pool.or([all, a]);
        assert_eq!(Bucket::classify(body, &pool), Bucket::DomainAbsorbable);
    }

    /// SOUNDNESS EXCLUSION 1. `Max(k, R, _)` with k ≥ 1 is `¬(≥k+1 R)`;
    /// absorbing it as a domain axiom fires at one successor instead of
    /// k+1 — strictly too strong. MUST NOT be `DomainAbsorbable`.
    #[test]
    fn max_k_ge_1_is_not_domain_absorbable() {
        let mut pool = ConceptPool::new();
        let top = pool.top();
        let a = pool.atomic(ClassId::new(0));
        for k in [1u32, 2, 7] {
            let m = pool.max(k, r0(), top);
            let body = pool.or([m, a]);
            let got = Bucket::classify(body, &pool);
            assert_ne!(
                got,
                Bucket::DomainAbsorbable,
                "Max({k},R,⊤) must not be domain-absorbable (unsound)"
            );
            assert_eq!(got, Bucket::CardAntecedentNGt1);
        }
    }

    /// SOUNDNESS EXCLUSION 2. `All(R, D)` with `D ≠ ⊥` is a *qualified*
    /// antecedent `∃R.¬D ⊑ rest`; it needs a filler check and does not
    /// reduce to a plain domain axiom. MUST NOT be `DomainAbsorbable`.
    #[test]
    fn all_non_bot_is_not_domain_absorbable() {
        let mut pool = ConceptPool::new();
        let d = pool.atomic(ClassId::new(1));
        let all = pool.all(r0(), d);
        let a = pool.atomic(ClassId::new(0));
        let body = pool.or([all, a]);
        let got = Bucket::classify(body, &pool);
        assert_ne!(got, Bucket::DomainAbsorbable);
        assert_eq!(got, Bucket::QualifiedExistsAntecedent);
    }

    /// `Max(0, R, C)` with `C ≠ ⊤` is `¬∃R.C` — the same qualified case
    /// written as a `≤0` cardinality. Also not a domain axiom.
    #[test]
    fn max_zero_qualified_is_not_domain_absorbable() {
        let mut pool = ConceptPool::new();
        let c = pool.atomic(ClassId::new(1));
        let m = pool.max(0, r0(), c);
        let a = pool.atomic(ClassId::new(0));
        let body = pool.or([m, a]);
        let got = Bucket::classify(body, &pool);
        assert_ne!(got, Bucket::DomainAbsorbable);
        assert_eq!(got, Bucket::QualifiedExistsAntecedent);
    }

    #[test]
    fn two_not_atomics_are_binary_absorbable() {
        let mut pool = ConceptPool::new();
        let a = pool.atomic(ClassId::new(0));
        let b = pool.atomic(ClassId::new(1));
        let c = pool.atomic(ClassId::new(2));
        let na = pool.not(a);
        let nb = pool.not(b);
        let body = pool.or([na, nb, c]);
        assert_eq!(Bucket::classify(body, &pool), Bucket::BinaryAbsorbable);
    }

    /// A single `Not(Atomic)` is what `as_trigger` already consumes; it is
    /// not a binary-absorption case.
    #[test]
    fn one_not_atomic_is_not_binary_absorbable() {
        let mut pool = ConceptPool::new();
        let a = pool.atomic(ClassId::new(0));
        let c = pool.atomic(ClassId::new(2));
        let na = pool.not(a);
        let body = pool.or([na, c]);
        assert_ne!(Bucket::classify(body, &pool), Bucket::BinaryAbsorbable);
    }

    #[test]
    fn not_nominal_is_nominal_absorbable() {
        let mut pool = ConceptPool::new();
        let n = pool.nominal(IndividualId::new(0));
        let nn = pool.not(n);
        let c = pool.atomic(ClassId::new(0));
        let body = pool.or([nn, c]);
        assert_eq!(Bucket::classify(body, &pool), Bucket::NominalAbsorbable);
    }

    #[test]
    fn plain_disjunction_is_genuinely_disjunctive() {
        let mut pool = ConceptPool::new();
        let a = pool.atomic(ClassId::new(0));
        let b = pool.atomic(ClassId::new(1));
        let body = pool.or([a, b]);
        assert_eq!(Bucket::classify(body, &pool), Bucket::GenuinelyDisjunctive);
    }

    /// Priority: domain wins over every less-specific bucket present in the
    /// same body. (`Max(0,R,⊤)` alongside `Max(3,S,⊤)` and two `¬Atomic`.)
    #[test]
    fn domain_wins_priority() {
        let mut pool = ConceptPool::new();
        let top = pool.top();
        let m0 = pool.max(0, r0(), top);
        let m3 = pool.max(3, Role::Named(RoleId::new(1)), top);
        let a = pool.atomic(ClassId::new(0));
        let b = pool.atomic(ClassId::new(1));
        let na = pool.not(a);
        let nb = pool.not(b);
        let body = pool.or([m0, m3, na, nb]);
        assert_eq!(Bucket::classify(body, &pool), Bucket::DomainAbsorbable);
    }

    /// A non-`Or` residual body is the singleton disjunct set.
    #[test]
    fn singleton_body_is_classified() {
        let mut pool = ConceptPool::new();
        let bot = pool.bot();
        let all = pool.all(r0(), bot);
        assert_eq!(Bucket::classify(all, &pool), Bucket::DomainAbsorbable);
    }

    /// Everything named, for the pool-only fixtures below.
    fn all_named(_: ClassId) -> bool {
        true
    }

    /// `D ≡ A ⊓ ∃r.F` ⇒ ⇐-direction conclusion `∀r.¬F ⊔ D`. The `∀r.¬F`
    /// disjunct says the body had an `∃r.F` conjunct, so `F ⊑ ∀r⁻.T` is
    /// derivable and a second guard is manufacturable.
    #[test]
    fn all_not_named_atomic_is_guard_manufacturable() {
        let mut pool = ConceptPool::new();
        let f = pool.atomic(ClassId::new(1));
        let nf = pool.not(f);
        let all = pool.all(r0(), nf);
        let d = pool.atomic(ClassId::new(2));
        let concl = pool.or([all, d]);
        let g = GuardManufacturability::classify(concl, &pool, &all_named);
        assert!(g.named, "∀r.¬F must be guard-manufacturable");
    }

    /// `=1 r.F` ⇒ `Min(1,r,F) ⊓ Max(1,r,F)`; the `≥` half negates to
    /// `Max(0,r,F)`, which qualifies (`≥1 r.F ⊨ ∃r.F`).
    #[test]
    fn max_zero_qualified_named_is_guard_manufacturable() {
        let mut pool = ConceptPool::new();
        let f = pool.atomic(ClassId::new(1));
        let m = pool.max(0, r0(), f);
        let d = pool.atomic(ClassId::new(2));
        let concl = pool.or([m, d]);
        let g = GuardManufacturability::classify(concl, &pool, &all_named);
        assert!(g.named, "Max(0,r,F) must be guard-manufacturable");
    }

    /// **NEGATIVE CONTROL — the whole point of the predicate.** `Min(k,r,F)`
    /// with `k ≥ 2` is the negation of the `≤(k-1) r.F` half of a
    /// cardinality conjunct. `≤n r.F` does **not** entail `∃r.F` (a node with
    /// no `r`-successor satisfies it), so nothing can be propagated backward
    /// and NO guard is manufacturable. A predicate counting every cardinality
    /// disjunct would report an addressable population that is too
    /// optimistic.
    #[test]
    fn min_k_ge_2_alone_is_not_guard_manufacturable() {
        let mut pool = ConceptPool::new();
        let f = pool.atomic(ClassId::new(1));
        let d = pool.atomic(ClassId::new(2));
        for k in [2u32, 3, 9] {
            let m = pool.min(k, r0(), f);
            let concl = pool.or([m, d]);
            let g = GuardManufacturability::classify(concl, &pool, &all_named);
            assert!(
                !g.named && !g.synthetic && !g.card_ge2 && !g.complex,
                "Min({k},r,F) alone must NOT be guard-manufacturable"
            );
        }
        // …and the census-level counter agrees.
        let m = pool.min(2, r0(), f);
        let concl = pool.or([m, d]);
        let tbox = AbsorbedTBox {
            concept_rules: vec![ConceptRule {
                trigger: ClassId::new(0),
                conclusion: concl,
            }],
            ..AbsorbedTBox::default()
        };
        let s = census(&tbox, &pool, None);
        assert_eq!(s.concept_rule_or, 1);
        assert_eq!(s.concept_rule_or_guard_manufacturable, 0);
        assert!(!s.all_or_rules_guard_manufacturable());
    }

    /// A rule may qualify via a *different* disjunct even when it also
    /// carries a non-qualifying `Min(k≥2)` one — exactly `KetoneGroup`'s
    /// shape (`=1 hDBW.O ⊓ =2 hSBW.CG` ⇒ `Max(0,hDBW,O) ⊔ Min(2,hDBW,O) ⊔
    /// Max(1,hSBW,CG) ⊔ Min(3,hSBW,CG)`).
    #[test]
    fn min_k_does_not_veto_a_qualifying_sibling() {
        let mut pool = ConceptPool::new();
        let f = pool.atomic(ClassId::new(1));
        let bad = pool.min(2, r0(), f);
        let good = pool.max(0, r0(), f);
        let d = pool.atomic(ClassId::new(2));
        let concl = pool.or([bad, good, d]);
        let g = GuardManufacturability::classify(concl, &pool, &all_named);
        assert!(g.named);
    }

    /// `Max(k≥1, r, F)` is the negation of `≥(k+1) r.F`, which DOES entail
    /// `∃r.F` — so it is semantically manufacturable, but it is held out of
    /// the headline counter and reported in its own field.
    #[test]
    fn card_ge2_is_held_out_of_the_headline() {
        let mut pool = ConceptPool::new();
        let f = pool.atomic(ClassId::new(1));
        let m = pool.max(1, r0(), f);
        let d = pool.atomic(ClassId::new(2));
        let concl = pool.or([m, d]);
        let g = GuardManufacturability::classify(concl, &pool, &all_named);
        assert!(!g.named, "Max(1,..) must not reach the headline");
        assert!(g.card_ge2);
        let tbox = AbsorbedTBox {
            concept_rules: vec![ConceptRule {
                trigger: ClassId::new(0),
                conclusion: concl,
            }],
            ..AbsorbedTBox::default()
        };
        let s = census(&tbox, &pool, None);
        assert_eq!(s.concept_rule_or_guard_manufacturable, 0);
        assert_eq!(s.concept_rule_or_guard_manufacturable_card_ge2_only, 1);
        assert_eq!(s.guard_manufacturable_any_tier(), 1);
    }

    /// TIER C. `∃r.(A ⊓ ∃s.B)` ⇒ `All(r, Or(¬A, All(s, ¬B)))`: a guard is
    /// manufacturable (`A ⊓ ∃s.B ⊑ ∀r⁻.T`) but that marker rule itself needs
    /// a multi-guard, recursive absorption — so it is held out of the
    /// headline and reported as complex-only.
    #[test]
    fn complex_filler_is_tier_c_not_the_headline() {
        let mut pool = ConceptPool::new();
        let a = pool.atomic(ClassId::new(1));
        let b = pool.atomic(ClassId::new(2));
        let na = pool.not(a);
        let nb = pool.not(b);
        let inner = pool.all(Role::Named(RoleId::new(1)), nb);
        let x = pool.or([na, inner]);
        let all = pool.all(r0(), x);
        let d = pool.atomic(ClassId::new(3));
        let concl = pool.or([all, d]);
        let g = GuardManufacturability::classify(concl, &pool, &all_named);
        assert!(!g.named, "a complex filler must not reach the headline");
        assert!(g.complex);
        let tbox = AbsorbedTBox {
            concept_rules: vec![ConceptRule {
                trigger: ClassId::new(0),
                conclusion: concl,
            }],
            ..AbsorbedTBox::default()
        };
        let s = census(&tbox, &pool, None);
        assert_eq!(s.concept_rule_or_guard_manufacturable, 0);
        assert_eq!(s.concept_rule_or_guard_manufacturable_complex_only, 1);
        assert_eq!(s.guard_manufacturable_any_tier(), 1);
        assert!(!s.all_or_rules_guard_manufacturable());
        assert!(s.all_or_rules_guard_manufacturable_any_tier());
    }

    /// `∀r.⊥` (= `¬∃r.⊤`) and `Max(k, r, ⊤)` yield NO class guard: the
    /// marker rule would be `⊤ ⊑ ∀r⁻.T`, which marks every node with an
    /// `r`-predecessor and guards nothing.
    #[test]
    fn top_filler_is_not_manufacturable_in_any_tier() {
        let mut pool = ConceptPool::new();
        let bot = pool.bot();
        let top = pool.top();
        let all_bot = pool.all(r0(), bot);
        let max_top = pool.max(0, r0(), top);
        let max_k_top = pool.max(2, r0(), top);
        let d = pool.atomic(ClassId::new(3));
        for disj in [all_bot, max_top, max_k_top] {
            let concl = pool.or([disj, d]);
            let g = GuardManufacturability::classify(concl, &pool, &all_named);
            assert_eq!(g, GuardManufacturability::default());
        }
    }

    /// A synthetic (`urn:rustdl-*`) filler is counted apart, never in the
    /// headline.
    #[test]
    fn synthetic_filler_is_counted_apart() {
        let mut pool = ConceptPool::new();
        let mut vocab = Vocabulary::new();
        let real = vocab.intern_class("http://example.org/F");
        let synth = vocab.intern_class("urn:rustdl-dkey:0:5");
        assert!(!is_synthetic_class_iri(vocab.class_iri(real)));
        assert!(is_synthetic_class_iri(vocab.class_iri(synth)));
        let sf = pool.atomic(synth);
        let m = pool.max(0, r0(), sf);
        let d = pool.atomic(ClassId::new(9));
        let concl = pool.or([m, d]);
        let tbox = AbsorbedTBox {
            concept_rules: vec![ConceptRule {
                trigger: ClassId::new(0),
                conclusion: concl,
            }],
            ..AbsorbedTBox::default()
        };
        let s = census(&tbox, &pool, Some(&vocab));
        assert_eq!(s.concept_rule_or_guard_manufacturable, 0);
        assert_eq!(s.concept_rule_or_guard_manufacturable_synthetic_only, 1);
    }

    /// Shared-trigger accounting: 3 rules on trigger 0, 2 on trigger 1, 1 on
    /// trigger 2 ⇒ two shared triggers, max 3, none at ≥5.
    #[test]
    fn shared_trigger_counts() {
        let mut pool = ConceptPool::new();
        let a = pool.atomic(ClassId::new(5));
        let b = pool.atomic(ClassId::new(6));
        let concl = pool.or([a, b]);
        let rule = |t: u32| ConceptRule {
            trigger: ClassId::new(t),
            conclusion: concl,
        };
        let tbox = AbsorbedTBox {
            concept_rules: vec![rule(0), rule(0), rule(0), rule(1), rule(1), rule(2)],
            ..AbsorbedTBox::default()
        };
        let s = census(&tbox, &pool, None);
        assert_eq!(s.concept_rule_or, 6);
        assert_eq!(s.distinct_shared_triggers, 2);
        assert_eq!(s.shared_triggers_ge5, 0);
        assert_eq!(s.max_disjunctive_rules_per_trigger, 3);
    }

    #[test]
    fn all_or_rules_manufacturable_predicate() {
        let mut pool = ConceptPool::new();
        let f = pool.atomic(ClassId::new(1));
        let good = pool.max(0, r0(), f);
        let d = pool.atomic(ClassId::new(2));
        let concl = pool.or([good, d]);
        let tbox = AbsorbedTBox {
            concept_rules: vec![ConceptRule {
                trigger: ClassId::new(0),
                conclusion: concl,
            }],
            ..AbsorbedTBox::default()
        };
        let s = census(&tbox, &pool, None);
        assert_eq!(s.concept_rule_or, 1);
        assert_eq!(s.concept_rule_or_guard_manufacturable, 1);
        assert!(s.all_or_rules_guard_manufacturable());
        // No Or-conclusion rules at all ⇒ vacuously false.
        let empty = census(&AbsorbedTBox::default(), &pool, None);
        assert!(!empty.all_or_rules_guard_manufacturable());
    }

    #[test]
    fn census_totals_and_concept_rule_or_counts() {
        let mut pool = ConceptPool::new();
        let top = pool.top();
        let a = pool.atomic(ClassId::new(0));
        let b = pool.atomic(ClassId::new(1));
        let c = pool.atomic(ClassId::new(2));
        let m0 = pool.max(0, r0(), top);
        let dom = pool.or([m0, a]);
        let plain = pool.or([a, b]);
        let m2 = pool.max(2, r0(), top);
        let card = pool.or([m2, a]);

        // concept rules: one Or-with-¬Atomic (binary-absorption candidate),
        // one plain Or, one non-Or.
        let nb = pool.not(b);
        let concl_binary = pool.or([nb, c]);
        let concl_plain = pool.or([b, c]);

        let tbox = AbsorbedTBox {
            residual_gcis: vec![dom, plain, card],
            concept_rules: vec![
                ConceptRule {
                    trigger: ClassId::new(0),
                    conclusion: concl_binary,
                },
                ConceptRule {
                    trigger: ClassId::new(1),
                    conclusion: concl_plain,
                },
                ConceptRule {
                    trigger: ClassId::new(2),
                    conclusion: c,
                },
            ],
            ..AbsorbedTBox::default()
        };
        let s = census(&tbox, &pool, None);
        assert_eq!(s.residual_gcis, 3);
        assert_eq!(s.domain_absorbable, 1);
        assert_eq!(s.genuinely_disjunctive, 1);
        assert_eq!(s.card_antecedent_n_gt_1, 1);
        assert_eq!(s.binary_absorbable, 0);
        assert_eq!(s.nominal_absorbable, 0);
        assert_eq!(s.qualified_exists_antecedent, 0);
        // buckets partition the residuals
        assert_eq!(
            s.domain_absorbable
                + s.binary_absorbable
                + s.nominal_absorbable
                + s.card_antecedent_n_gt_1
                + s.qualified_exists_antecedent
                + s.genuinely_disjunctive,
            s.residual_gcis
        );
        assert_eq!(s.concept_rules, 3);
        assert_eq!(s.concept_rule_or, 2);
        assert_eq!(s.concept_rule_or_with_extra_not_atomic, 1);
        assert_eq!(s.removed_by_domain(), 1);
        assert_eq!(s.removed_by_domain_and_binary(), 1);
        assert!(!s.zero_residuals_under_domain());
    }

    #[test]
    fn zero_residual_predicates() {
        let mut pool = ConceptPool::new();
        let top = pool.top();
        let a = pool.atomic(ClassId::new(0));
        let m0 = pool.max(0, r0(), top);
        let dom = pool.or([m0, a]);
        let tbox = AbsorbedTBox {
            residual_gcis: vec![dom, dom],
            ..AbsorbedTBox::default()
        };
        let s = census(&tbox, &pool, None);
        assert!(s.zero_residuals_under_domain());
        assert!(s.zero_residuals_under_domain_and_binary());
    }
}
