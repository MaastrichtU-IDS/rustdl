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
use crate::ir::{ConceptExpr, ConceptId, ConceptPool};

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
#[must_use]
pub fn census(tbox: &AbsorbedTBox, pool: &ConceptPool) -> ResidualAbsorbabilityStats {
    let mut stats = ResidualAbsorbabilityStats {
        residual_gcis: tbox.residual_gcis.len(),
        concept_rules: tbox.concept_rules.len(),
        ..ResidualAbsorbabilityStats::default()
    };
    for &r in &tbox.residual_gcis {
        stats.bump(Bucket::classify(r, pool));
    }
    for rule in &tbox.concept_rules {
        if let ConceptExpr::Or(args) = pool.get(rule.conclusion) {
            stats.concept_rule_or += 1;
            let extra = args.iter().any(|&d| {
                matches!(pool.get(d), ConceptExpr::Not(inner)
                    if matches!(pool.get(*inner), ConceptExpr::Atomic(_)))
            });
            if extra {
                stats.concept_rule_or_with_extra_not_atomic += 1;
            }
        }
    }
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
        let s = census(&tbox, &pool);
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
        let s = census(&tbox, &pool);
        assert!(s.zero_residuals_under_domain());
        assert!(s.zero_residuals_under_domain_and_binary());
    }
}
