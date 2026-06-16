//! Proof recording and extraction for the EL saturator.
//!
//! When `RUSTDL_PROOF=1` (or `SaturateConfig { record_proofs: true }`), the
//! saturator maintains a side-table (`ProofTrace`) recording, for each derived
//! fact, the rule that produced it and the premises it consumed. The table is
//! **write-only** from the saturation perspective — it is never read back during
//! closure computation, so verdicts are byte-identical to the non-recording path.
//!
//! After saturation, `prove_subsumption` walks the side-table backward from a
//! `Sub(sub, sup)` fact to the ontology-axiom leaves and returns a `ProofNode`
//! tree. For entailments outside the EL saturation fragment the caller should
//! use the `JustificationFallback` path (black-box axiom-set justification from
//! the shipped `owl_dl_reasoner::justify` module).
//!
//! The `check_proof` function re-verifies each recorded step against the rule's
//! semantic definition. It is cheap (one pass, no fixpoint) and runs only in
//! tests and on the `prove --verify-proof` flag.

use std::collections::HashMap;
use std::hash::BuildHasher;

use owl_dl_core::{ClassId, IndividualId, RoleId};

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// A derived fact produced by the EL saturator.
///
/// Used as the key in `ProofTrace::steps`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum DerivedFact {
    /// A subsumer edge: `sub ⊑ sup`.
    Sub(ClassId, ClassId),
    /// An existential fact: `sub ⊑ ∃role.target`.
    Exist(ClassId, RoleId, ClassId),
    /// Unsatisfiability: `class ⊑ ⊥`.
    Unsat(ClassId),
}

/// A reference to an ontology axiom by index into `InternalOntology::axioms`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AxiomRef(pub usize);

/// Which EL rule produced a derived fact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ElRule {
    /// `C ⊑ C` seeded at start (no axiom).
    Reflexivity,
    /// Direct `A ⊑ B` from `SubClassOf(A,B)` or `EquivalentClasses`.
    ToldSubsumer,
    /// Direct `A ⊑ ∃R.B` from `SubClassOf(A, ∃R.B)` or equivalent.
    ToldFact,
    /// Phase D4: `A ⊑ ⊥` from data-axiom preprocessing.
    ToldUnsat,
    /// Transitivity of ⊑ (forward): `A ⊑ B`, `B ⊑ C` ⟹ `A ⊑ C`.
    SubsumerTransitivityFwd,
    /// Transitivity of ⊑ (backward): `X ⊑ C`, `C ⊑ D` ⟹ `X ⊑ D`.
    SubsumerTransitivityBwd,
    /// Unsat propagation from subsumer: `C ⊑ D`, `D ⊑ ⊥` ⟹ `C ⊑ ⊥`.
    UnsatSubsumer,
    /// Conjunctive trigger: all `Bᵢ ∈ supers(C)` ⟹ `C ⊑ head`.
    ConjunctiveTrigger,
    /// Disjointness → unsat: `C ⊑ A`, `C ⊑ B`, `Disjoint(A,B)` ⟹ `C ⊑ ⊥`.
    DisjointnessClash,
    /// CR5 existential trigger (target-side): new subsumer on the target fires triggers.
    ExistentialTriggerTarget,
    /// CR5 existential trigger (sub-side): new subsumer drives fact lookup + triggers.
    ExistentialTriggerSub,
    /// Domain axiom (subsumer-side): `C ⊑ D`, `(D,r,_)` fact, `Domain(r,dom)` ⟹ `C ⊑ dom`.
    DomainSub,
    /// Phase 2d fact inheritance: `C ⊑ D`, `(D,r,T)` ⟹ `(C,r,T)`.
    FactInheritance,
    /// Role chain (subsumer-triggered): new subsumer on chain middle node.
    RoleChainSubsumer,
    /// Cluster-B `MaxKey`: `C ⊑ ≤1 R` + `∃R.{a}`, a∈S gives `C ⊑ ForallKey(R,S)`.
    MaxKeyDerived,
    /// `ABox` nominal transitive propagation: `X ⊑ ∃R.{a}`, aR+b gives `X ⊑ ∃R.{b}`.
    NominalTransitiveProp,
    /// Cluster-B `ForallKey`: functional R + `∃R.{a}`, a∈S gives `C ⊑ ForallKey(R,S)`.
    ForallKeyDerived,
    /// Domain axiom (fact-side): `(sub,r,_)`, `Domain(r,dom)` ⟹ `sub ⊑ dom`.
    DomainFact,
    /// Unsat propagation from existential target: `(C,r,T)`, `T ⊑ ⊥` ⟹ `C ⊑ ⊥`.
    UnsatTarget,
    /// CR5 existential trigger (fact-triggered): new fact arrives, fires trigger.
    ExistentialTriggerFact,
    /// Role chain (fact head/tail pairing).
    RoleChainFact,
    /// Phase 2a functional witness-merge: multiple sub-role facts → merged synthetic.
    FunctionalMerge,
    /// Phase 2c-redux: merged synthetic back-propagated to sub-role.
    FunctionalMergeSubRole,
    /// Unsat propagation (`process_unsat`, subclass): `D ⊑ C`, `C ⊑ ⊥` ⟹ `D ⊑ ⊥`.
    UnsatSubclass,
    /// Unsat propagation (`process_unsat`, fact-source): `(D,r,C)`, `C ⊑ ⊥` ⟹ `D ⊑ ⊥`.
    UnsatFactSource,
}

/// One step in a proof: how a derived fact was produced.
#[derive(Debug, Clone)]
pub struct Inference {
    /// Which EL rule fired.
    pub rule: ElRule,
    /// Premises: prior derived facts this step consumed (empty for axiom leaves).
    pub premise_facts: Vec<DerivedFact>,
    /// The axiom(s) that justify this step (empty for pure transitivity / chain).
    pub axiom_refs: Vec<AxiomRef>,
}

/// The provenance side-table for synthetic class definitions, used for rendering.
#[derive(Debug, Clone)]
pub enum SyntheticDef {
    /// Tseitin: `F ≡ B₁⊓…⊓Bₙ`; bodies are `ClassId`s (may themselves be synthetic).
    TseitinConj(Vec<ClassId>),
    /// Existential marker (one-way): `∃R.B ⊑ M`.
    ExistMarkerOneWay { role: RoleId, body: ClassId },
    /// Existential marker (two-way): `M ≡ ∃R.B`.
    ExistMarkerEquiv { role: RoleId, body: ClassId },
    /// Nominal key: stands in for individual `{a}`.
    NominalKey(IndividualId),
    /// `MaxKey`: stands in for `≤n R`.
    MaxKey { n: u32, role: RoleId },
    /// `ForallKey`: stands in for `∀R.OneOf(S)`.
    ForallKey {
        role: RoleId,
        members: Vec<IndividualId>,
    },
    /// `DKey` (datatype): stands in for a datatype range interval. IRI suffix identifies bucket.
    DKey(String),
}

/// Side-table mapping every derived fact to the inference that first produced it.
///
/// **First-writer-wins**: when multiple rules could derive the same fact, only the
/// first one is recorded. This is sound (any valid derivation path is a proof) and
/// guarantees the DAG is acyclic (premises were derived strictly earlier).
#[derive(Debug, Default)]
pub struct ProofTrace {
    /// Per-fact inference record.
    pub steps: HashMap<DerivedFact, Inference>,
    /// Synthetic class definitions, for human-readable rendering.
    pub synthetic_defs: HashMap<ClassId, SyntheticDef>,
    /// Axiom provenance for atomic-subsumption rules: parallel to
    /// `ElRules::atomic_subsumptions`. Index `i` → the axiom index that
    /// introduced `atomic_subsumptions[i]`. Populated during `collect_el_rules`.
    pub(crate) atomic_sub_axiom: Vec<Option<usize>>,
    /// Axiom provenance for existential facts: parallel to `ElRules::existential_facts`.
    pub(crate) existential_fact_axiom: Vec<Option<usize>>,
    /// Axiom provenance for conjunctive triggers.
    pub(crate) conjunctive_trigger_axiom: Vec<Option<usize>>,
    /// Axiom provenance for existential triggers.
    pub(crate) existential_trigger_axiom: Vec<Option<usize>>,
    /// Axiom provenance for disjoint pairs.
    pub(crate) disjoint_pair_axiom: Vec<Option<usize>>,
    /// Axiom provenance for chain axioms.
    pub(crate) chain_axiom_axiom: Vec<Option<usize>>,
    /// Axiom provenance for role domains: `(role_id, domain_class, axiom_idx)` triples.
    pub(crate) domain_axiom_refs: Vec<(RoleId, ClassId, usize)>,
    /// Axiom provenance for `directly_unsat` classes.
    pub(crate) directly_unsat_axiom: Vec<Option<usize>>,
    /// Axiom provenance for functional roles: `role_id → axiom_idx`.
    pub(crate) functional_role_axiom: HashMap<RoleId, usize>,
    /// For each (sub, rf) key, the list of contributing `DerivedFact::Exist` entries
    /// that have been merged into the functional witness for `rf`. Used by R21.
    pub(crate) merge_contributors: HashMap<(ClassId, RoleId), Vec<DerivedFact>>,
    /// `ABox` path table: `(role, from_nom_key, to_nom_key)` → `Vec<AxiomRef>`.
    pub(crate) abox_path: HashMap<(RoleId, ClassId, ClassId), Vec<AxiomRef>>,
}

impl ProofTrace {
    /// Record an inference step (first-writer-wins).
    pub(crate) fn record(&mut self, fact: DerivedFact, inf: Inference) {
        self.steps.entry(fact).or_insert(inf);
    }
}

// ---------------------------------------------------------------------------
// Proof node (extracted proof tree)
// ---------------------------------------------------------------------------

/// A node in the extracted proof DAG.
#[derive(Debug, Clone)]
pub struct ProofNode {
    /// The fact this node proves.
    pub conclusion: DerivedFact,
    /// The rule that derived it.
    pub rule: ElRule,
    /// The axiom refs used by this step.
    pub axiom_refs: Vec<AxiomRef>,
    /// The premise sub-proofs.
    pub premises: Vec<ProofNode>,
}

/// The result of a `prove_subsumption` call.
#[derive(Debug)]
pub enum ProveResult {
    /// Step-level proof from the EL saturator.
    SaturatorProof(ProofNode),
    /// The entailment is sound but not in the saturation fragment.
    /// The inner value is `None` when the trace has no record for the fact
    /// (the caller should fall back to black-box justification).
    NotInSaturationFragment,
    /// The entailment is not held by the ontology.
    NotEntailed,
}

/// Walk the `ProofTrace` backward from `DerivedFact::Sub(sub, sup)` to the
/// ontology-axiom leaves.
///
/// Returns `None` if `Sub(sub, sup)` is not in `trace.steps` — the pair was not
/// derived by the saturator (out-of-fragment or not entailed from this path).
///
/// **Memoization:** the `memo` map avoids exponential re-expansion of shared
/// sub-derivations. Pass `&mut HashMap::new()` on the first call.
#[must_use]
pub fn prove_subsumption<S: BuildHasher>(
    trace: &ProofTrace,
    sub: ClassId,
    sup: ClassId,
    memo: &mut HashMap<DerivedFact, ProofNode, S>,
) -> Option<ProofNode> {
    prove_fact(trace, &DerivedFact::Sub(sub, sup), memo)
}

fn prove_fact<S: BuildHasher>(
    trace: &ProofTrace,
    fact: &DerivedFact,
    memo: &mut HashMap<DerivedFact, ProofNode, S>,
) -> Option<ProofNode> {
    if let Some(cached) = memo.get(fact) {
        return Some(cached.clone());
    }
    let inf = trace.steps.get(fact)?;
    let premise_nodes: Vec<ProofNode> = inf
        .premise_facts
        .iter()
        .filter_map(|p| prove_fact(trace, p, memo))
        .collect();
    // If we couldn't prove all premises (recording gap), include what we have.
    let node = ProofNode {
        conclusion: fact.clone(),
        rule: inf.rule.clone(),
        axiom_refs: inf.axiom_refs.clone(),
        premises: premise_nodes,
    };
    memo.insert(fact.clone(), node.clone());
    Some(node)
}

// ---------------------------------------------------------------------------
// Faithfulness checker
// ---------------------------------------------------------------------------

/// Error type for proof-checker failures.
#[derive(Debug)]
pub struct CheckError {
    pub fact: DerivedFact,
    pub message: String,
}

impl std::fmt::Display for CheckError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "proof check failed for {:?}: {}",
            self.fact, self.message
        )
    }
}

/// Re-verify that each `ProofNode` is a valid instance of its stated rule.
///
/// Recurses into premises. Axiom-ref range checks require `num_axioms` (the
/// length of `InternalOntology::axioms`).
///
/// # Errors
/// Returns the first `CheckError` found.
pub fn check_proof(node: &ProofNode, num_axioms: usize) -> Result<(), CheckError> {
    // Check axiom refs are in range.
    for ax in &node.axiom_refs {
        if ax.0 >= num_axioms {
            return Err(CheckError {
                fact: node.conclusion.clone(),
                message: format!("axiom ref {} out of range (have {})", ax.0, num_axioms),
            });
        }
    }
    // Rule-specific semantic checks.
    match &node.rule {
        ElRule::Reflexivity => {
            // C ⊑ C — conclusion must be Sub(C,C).
            if let DerivedFact::Sub(sub, sup) = &node.conclusion {
                if sub != sup {
                    return Err(CheckError {
                        fact: node.conclusion.clone(),
                        message: "Reflexivity: sub ≠ sup".to_string(),
                    });
                }
            } else {
                return Err(CheckError {
                    fact: node.conclusion.clone(),
                    message: "Reflexivity: conclusion is not Sub".to_string(),
                });
            }
        }
        ElRule::SubsumerTransitivityFwd => {
            // Sub(C,D) + Sub(D,E) ⟹ Sub(C,E)
            if let DerivedFact::Sub(c, e) = &node.conclusion {
                if node.premises.len() != 2 {
                    return Err(CheckError {
                        fact: node.conclusion.clone(),
                        message: format!(
                            "SubsumerTransitivityFwd: expected 2 premises, got {}",
                            node.premises.len()
                        ),
                    });
                }
                let DerivedFact::Sub(p0_sub, p0_sup) = &node.premises[0].conclusion else {
                    return Err(CheckError {
                        fact: node.conclusion.clone(),
                        message: "SubsumerTransitivityFwd: premise[0] not Sub".to_string(),
                    });
                };
                let DerivedFact::Sub(p1_sub, p1_sup) = &node.premises[1].conclusion else {
                    return Err(CheckError {
                        fact: node.conclusion.clone(),
                        message: "SubsumerTransitivityFwd: premise[1] not Sub".to_string(),
                    });
                };
                // Sub(C,D) + Sub(D,E) ⟹ Sub(C,E)
                if p0_sub != c || p1_sup != e || p0_sup != p1_sub {
                    return Err(CheckError {
                        fact: node.conclusion.clone(),
                        message: format!(
                            "SubsumerTransitivityFwd: premises {p0_sub:?}⊑{p0_sup:?}, {p1_sub:?}⊑{p1_sup:?} don't chain to {c:?}⊑{e:?}"
                        ),
                    });
                }
            } else {
                return Err(CheckError {
                    fact: node.conclusion.clone(),
                    message: "SubsumerTransitivityFwd: conclusion is not Sub".to_string(),
                });
            }
        }
        ElRule::SubsumerTransitivityBwd => {
            // Sub(X,C) + Sub(C,D) ⟹ Sub(X,D)
            if let DerivedFact::Sub(x, d) = &node.conclusion {
                if node.premises.len() != 2 {
                    return Err(CheckError {
                        fact: node.conclusion.clone(),
                        message: format!(
                            "SubsumerTransitivityBwd: expected 2 premises, got {}",
                            node.premises.len()
                        ),
                    });
                }
                let DerivedFact::Sub(p0_sub, p0_sup) = &node.premises[0].conclusion else {
                    return Err(CheckError {
                        fact: node.conclusion.clone(),
                        message: "SubsumerTransitivityBwd: premise[0] not Sub".to_string(),
                    });
                };
                let DerivedFact::Sub(p1_sub, p1_sup) = &node.premises[1].conclusion else {
                    return Err(CheckError {
                        fact: node.conclusion.clone(),
                        message: "SubsumerTransitivityBwd: premise[1] not Sub".to_string(),
                    });
                };
                // Sub(X,C) + Sub(C,D) ⟹ Sub(X,D)
                if p0_sub != x || p1_sup != d || p0_sup != p1_sub {
                    return Err(CheckError {
                        fact: node.conclusion.clone(),
                        message: format!(
                            "SubsumerTransitivityBwd: premises {p0_sub:?}⊑{p0_sup:?}, {p1_sub:?}⊑{p1_sup:?} don't chain to {x:?}⊑{d:?}"
                        ),
                    });
                }
            } else {
                return Err(CheckError {
                    fact: node.conclusion.clone(),
                    message: "SubsumerTransitivityBwd: conclusion is not Sub".to_string(),
                });
            }
        }
        ElRule::ToldSubsumer | ElRule::ToldFact | ElRule::ToldUnsat => {
            // Axiom-leaf: no premises, axiom_refs validated above.
        }
        ElRule::UnsatSubsumer => {
            // Sub(C,D) + Unsat(D) ⟹ Unsat(C)
            if let DerivedFact::Unsat(c) = &node.conclusion {
                let has_sub_premise = node
                    .premises
                    .iter()
                    .any(|p| matches!(&p.conclusion, DerivedFact::Sub(s, _) if s == c));
                let has_unsat_premise = node
                    .premises
                    .iter()
                    .any(|p| matches!(&p.conclusion, DerivedFact::Unsat(_)));
                if !has_sub_premise || !has_unsat_premise {
                    return Err(CheckError {
                        fact: node.conclusion.clone(),
                        message: "UnsatSubsumer: missing Sub or Unsat premise".to_string(),
                    });
                }
            } else {
                return Err(CheckError {
                    fact: node.conclusion.clone(),
                    message: "UnsatSubsumer: conclusion is not Unsat".to_string(),
                });
            }
        }
        ElRule::UnsatTarget => {
            // Exist(C,r,T) + Unsat(T) ⟹ Unsat(C)
            if let DerivedFact::Unsat(c) = &node.conclusion {
                let has_exist = node
                    .premises
                    .iter()
                    .any(|p| matches!(&p.conclusion, DerivedFact::Exist(s, _, _) if s == c));
                let has_unsat = node
                    .premises
                    .iter()
                    .any(|p| matches!(&p.conclusion, DerivedFact::Unsat(_)));
                if !has_exist || !has_unsat {
                    return Err(CheckError {
                        fact: node.conclusion.clone(),
                        message: "UnsatTarget: missing Exist or Unsat premise".to_string(),
                    });
                }
            } else {
                return Err(CheckError {
                    fact: node.conclusion.clone(),
                    message: "UnsatTarget: conclusion is not Unsat".to_string(),
                });
            }
        }
        // Other rules: coarse check — at least one premise or one axiom ref.
        _ => {
            if node.premises.is_empty() && node.axiom_refs.is_empty() {
                // Relaxed: some rules (e.g. NominalTransitiveProp) may have no
                // recoverable premise path. Accept as a coarse-but-correct step.
                // Only error if this is a rule that should always have premises.
                let requires_premise = matches!(
                    node.rule,
                    ElRule::ConjunctiveTrigger
                        | ElRule::ExistentialTriggerTarget
                        | ElRule::ExistentialTriggerSub
                        | ElRule::ExistentialTriggerFact
                        | ElRule::FactInheritance
                        | ElRule::RoleChainSubsumer
                        | ElRule::RoleChainFact
                        | ElRule::FunctionalMerge
                        | ElRule::FunctionalMergeSubRole
                        | ElRule::UnsatSubclass
                        | ElRule::UnsatFactSource
                );
                if requires_premise {
                    return Err(CheckError {
                        fact: node.conclusion.clone(),
                        message: format!("{:?}: no premises and no axiom refs", node.rule),
                    });
                }
            }
        }
    }
    // Recurse into premises.
    for premise in &node.premises {
        check_proof(premise, num_axioms)?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Rendering helpers
// ---------------------------------------------------------------------------

impl std::fmt::Display for DerivedFact {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DerivedFact::Sub(s, p) => write!(f, "Sub({s:?}, {p:?})"),
            DerivedFact::Exist(s, r, t) => write!(f, "Exist({s:?}, {r:?}, {t:?})"),
            DerivedFact::Unsat(c) => write!(f, "Unsat({c:?})"),
        }
    }
}

impl std::fmt::Display for ElRule {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            ElRule::Reflexivity => "Reflexivity",
            ElRule::ToldSubsumer => "ToldSubsumer",
            ElRule::ToldFact => "ToldFact",
            ElRule::ToldUnsat => "ToldUnsat",
            ElRule::SubsumerTransitivityFwd => "SubsumerTransitivity(fwd)",
            ElRule::SubsumerTransitivityBwd => "SubsumerTransitivity(bwd)",
            ElRule::UnsatSubsumer => "UnsatSubsumer",
            ElRule::ConjunctiveTrigger => "ConjunctiveTrigger",
            ElRule::DisjointnessClash => "DisjointnessClash",
            ElRule::ExistentialTriggerTarget => "ExistentialTrigger(target)",
            ElRule::ExistentialTriggerSub => "ExistentialTrigger(sub)",
            ElRule::DomainSub => "Domain(sub)",
            ElRule::FactInheritance => "FactInheritance",
            ElRule::RoleChainSubsumer => "RoleChain(subsumer)",
            ElRule::MaxKeyDerived => "MaxKeyDerived",
            ElRule::NominalTransitiveProp => "NominalTransitiveProp",
            ElRule::ForallKeyDerived => "ForallKeyDerived",
            ElRule::DomainFact => "Domain(fact)",
            ElRule::UnsatTarget => "UnsatTarget",
            ElRule::ExistentialTriggerFact => "ExistentialTrigger(fact)",
            ElRule::RoleChainFact => "RoleChain(fact)",
            ElRule::FunctionalMerge => "FunctionalMerge",
            ElRule::FunctionalMergeSubRole => "FunctionalMerge(sub-role)",
            ElRule::UnsatSubclass => "UnsatSubclass",
            ElRule::UnsatFactSource => "UnsatFactSource",
        };
        write!(f, "{name}")
    }
}

/// Render a `ProofNode` tree to a string (depth-first, indented).
///
/// Each line is: `{indent}({rule}) {conclusion}`
/// followed by axiom refs and then recursively the premises.
#[must_use]
pub fn render_proof(
    node: &ProofNode,
    vocab: Option<&owl_dl_core::Vocabulary>,
    indent: usize,
) -> String {
    let pad = "  ".repeat(indent);
    let concl = render_fact(&node.conclusion, vocab);
    let rule_str = node.rule.to_string();
    let mut out = format!("{pad}({rule_str}) ⊢ {concl}");
    if !node.axiom_refs.is_empty() {
        let refs: Vec<String> = node
            .axiom_refs
            .iter()
            .map(|r| format!("axiom[{}]", r.0))
            .collect();
        out.push_str("  [");
        out.push_str(&refs.join(", "));
        out.push(']');
    }
    out.push('\n');
    for premise in &node.premises {
        out.push_str(&render_proof(premise, vocab, indent + 1));
    }
    out
}

fn render_fact(fact: &DerivedFact, vocab: Option<&owl_dl_core::Vocabulary>) -> String {
    match fact {
        DerivedFact::Sub(s, p) => {
            let s_str = render_class(*s, vocab);
            let p_str = render_class(*p, vocab);
            format!("{s_str} ⊑ {p_str}")
        }
        DerivedFact::Exist(s, r, t) => {
            let s_str = render_class(*s, vocab);
            let r_str = render_role(*r, vocab);
            let t_str = render_class(*t, vocab);
            format!("{s_str} ⊑ ∃{r_str}.{t_str}")
        }
        DerivedFact::Unsat(c) => {
            let c_str = render_class(*c, vocab);
            format!("{c_str} ⊑ ⊥")
        }
    }
}

fn render_class(id: ClassId, vocab: Option<&owl_dl_core::Vocabulary>) -> String {
    vocab.map_or_else(
        || format!("{id:?}"),
        |v| {
            let idx = id.index() as usize;
            if idx < v.num_classes() {
                v.class_iri(id).to_string()
            } else {
                format!("synthetic#{idx}")
            }
        },
    )
}

fn render_role(id: RoleId, vocab: Option<&owl_dl_core::Vocabulary>) -> String {
    vocab.map_or_else(
        || format!("{id:?}"),
        |v| {
            let idx = id.index() as usize;
            if idx < v.num_roles() {
                v.role_iri(id).to_string()
            } else {
                format!("role#{idx}")
            }
        },
    )
}
