//! Told definition matching (#139): `A ⊑ X₁ ⊓ … ⊓ Xₙ`, `D ≡ X₁ ⊓ … ⊓ Xₙ`
//! ⟹ `A ⊑ D`, by interned identity of the conjuncts.
//!
//! The motivating case is two axioms in the PMD core ontology:
//! `SubClassOf(PMD_0010100, =1 RO_0002351.PMD_0020140)` and
//! `EquivalentClasses(PMD_0020210, =1 RO_0002351.PMD_0020140)`. The entailment
//! needs no reasoning beyond matching the two expressions, but `=1` is outside the
//! EL saturator's fragment, so it was left to a per-pair tableau probe — which
//! inside the full ontology does not finish in 120 s, and `classify` missed it plus
//! the two superclasses it implies. Emitting the atomic `A ⊑ D` closes all three.
//!
//! **Rule.** For a definition `D ≡ Y` with `Y` non-atomic and conjuncts `c(Y)`
//! (`Y` itself unless it is an `And`), derive `A ⊑ D` when every conjunct is
//! satisfied for `A`:
//! - a complex conjunct `k` is satisfied when some told superclass `B` of `A`
//!   (including `A`) asserts `k`: `SubClassOf(B, k)`, `EquivalentClasses(B, k)`,
//!   or `k` is a conjunct of such an asserted `And`;
//! - an atomic conjunct `K` is satisfied when `K` is a told superclass of `A`, or
//!   of an atomic conjunct asserted on a told superclass of `A`.
//!
//! **Sound by construction**: every step is a told subsumption or interned
//! syntactic identity, so each emitted axiom is entailed. It only adds atomic
//! `SubClassOf` axioms. Only definitions with at least one conjunct OUTSIDE EL
//! (anything but `⊤`, atomic, `⊓` and forward `∃` over such) are matched: when
//! every conjunct is EL, the EL saturator already derives the edge from the EL
//! part of the ontology. Without that filter the pass cost tens of seconds on
//! large EL ontologies (the PRO-derived ORE members, each with ~30,000 EL
//! definitions) and found nothing the saturator does not. With no qualifying
//! definition the told tables are never built, so the pass is a single axiom scan.
//!
//! Candidates visited per round are capped at [`MAX_CHECKS`]; like the round cap,
//! stopping early only loses derivations.
//!
//! Runs to a fixpoint (each new edge can enable another match), capped at
//! [`MAX_ROUNDS`]; stopping early only loses derivations.
//!
//! `RUSTDL_TOLD_DEFINITION_MATCH` — **default ON**, `=0` disables.

use crate::ir::{ClassId, ConceptExpr, ConceptId, ConceptPool};
use crate::ontology::{Axiom, InternalOntology};
use crate::told::build_told_tables;
use std::collections::{HashMap, HashSet};

/// Rounds of the fixpoint before giving up (sound either way).
pub const MAX_ROUNDS: usize = 8;

/// Candidates visited per round before giving up on the round (sound either way).
pub const MAX_CHECKS: usize = 2_000_000;

/// See the module doc. Default ON; `RUSTDL_TOLD_DEFINITION_MATCH=0` disables.
#[must_use]
pub fn told_definition_match_enabled() -> bool {
    std::env::var_os("RUSTDL_TOLD_DEFINITION_MATCH").is_none_or(|v| v != "0")
}

fn atomic(cid: ConceptId, pool: &ConceptPool) -> Option<ClassId> {
    match pool.get(cid) {
        ConceptExpr::Atomic(c) => Some(*c),
        _ => None,
    }
}

/// `⊤`, atomic, `⊓` and forward `∃` over such: what the EL saturator handles.
fn is_el(cid: ConceptId, pool: &ConceptPool) -> bool {
    match pool.get(cid) {
        ConceptExpr::Top | ConceptExpr::Atomic(_) => true,
        ConceptExpr::And(ops) => ops.iter().all(|&op| is_el(op, pool)),
        ConceptExpr::Some(role, body) => !role.is_inverse() && is_el(*body, pool),
        _ => false,
    }
}

/// The conjuncts of `cid`: its operands if it is an `And`, else itself.
fn conjuncts(cid: ConceptId, pool: &ConceptPool) -> Vec<ConceptId> {
    match pool.get(cid) {
        ConceptExpr::And(cs) => cs.to_vec(),
        _ => vec![cid],
    }
}

/// A definition `D ≡ Y` split into its atomic and complex conjuncts.
struct Definition {
    class: ClassId,
    atoms: Vec<ClassId>,
    complex: Vec<ConceptId>,
}

/// Appends every derivable `A ⊑ D` as an atomic `SubClassOf`. Returns how many
/// axioms were added.
pub fn derive_told_definition_matches(onto: &mut InternalOntology) -> usize {
    if !told_definition_match_enabled() {
        return 0;
    }
    let n = onto.vocabulary.num_classes();
    let mut added = 0;
    for _ in 0..MAX_ROUNDS {
        let pool = &onto.concepts;

        // What each class asserts directly: complex conjuncts, atomic conjuncts.
        let mut asserted: Vec<HashSet<ConceptId>> = vec![HashSet::new(); n];
        let mut asserted_atoms: Vec<Vec<ClassId>> = vec![Vec::new(); n];
        let mut defs: Vec<Definition> = Vec::new();
        let mut assert_on = |a: ClassId, sup: ConceptId| {
            for k in conjuncts(sup, pool) {
                match pool.get(k) {
                    ConceptExpr::Atomic(c) => asserted_atoms[a.index() as usize].push(*c),
                    ConceptExpr::Top => {}
                    _ => {
                        asserted[a.index() as usize].insert(k);
                    }
                }
            }
        };
        for ax in &onto.axioms {
            match ax {
                Axiom::SubClassOf { sub, sup } => {
                    if let Some(a) = atomic(*sub, pool)
                        && atomic(*sup, pool).is_none()
                    {
                        assert_on(a, *sup);
                    }
                }
                Axiom::EquivalentClasses(ids) => {
                    for &d in ids {
                        let Some(dc) = atomic(d, pool) else { continue };
                        for &y in ids {
                            if atomic(y, pool).is_some() {
                                continue;
                            }
                            assert_on(dc, y);
                            let (mut atoms, mut complex) = (Vec::new(), Vec::new());
                            for k in conjuncts(y, pool) {
                                match pool.get(k) {
                                    ConceptExpr::Atomic(c) => atoms.push(*c),
                                    ConceptExpr::Top => {}
                                    _ => complex.push(k),
                                }
                            }
                            if complex.iter().any(|&k| !is_el(k, pool))
                                && !matches!(pool.get(y), ConceptExpr::Bot)
                            {
                                defs.push(Definition {
                                    class: dc,
                                    atoms,
                                    complex,
                                });
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        if defs.is_empty() {
            break;
        }
        let told = build_told_tables(onto);

        // Index: complex conjunct -> classes asserting it directly.
        let mut by_conjunct: HashMap<ConceptId, Vec<ClassId>> = HashMap::new();
        for (i, set) in asserted.iter().enumerate() {
            for &k in set {
                by_conjunct
                    .entry(k)
                    .or_default()
                    .push(ClassId::new(u32::try_from(i).unwrap_or(u32::MAX)));
            }
        }

        let has_complex = |a: ClassId, k: ConceptId| {
            told.super_classes(a)
                .iter()
                .any(|b| asserted[b.index() as usize].contains(&k))
        };
        let has_atom = |a: ClassId, k: ClassId| {
            let sups = told.super_classes(a);
            sups.binary_search(&k).is_ok()
                || sups.iter().any(|b| {
                    asserted_atoms[b.index() as usize]
                        .iter()
                        .any(|&c| told.super_classes(c).binary_search(&k).is_ok())
                })
        };

        let mut new_edges: Vec<(ClassId, ClassId)> = Vec::new();
        let mut seen: HashSet<(ClassId, ClassId)> = HashSet::new();
        let mut checks = 0usize;
        'defs: for def in &defs {
            // Seed candidates from the rarest complex conjunct.
            let Some(seeds) = def
                .complex
                .iter()
                .filter_map(|k| by_conjunct.get(k))
                .min_by_key(|v| v.len())
            else {
                continue;
            };
            if def.complex.iter().any(|k| !by_conjunct.contains_key(k)) {
                continue;
            }
            for &a0 in seeds {
                for &a in told.sub_classes(a0) {
                    checks += 1;
                    if checks > MAX_CHECKS {
                        break 'defs;
                    }
                    if a == def.class
                        || told.super_classes(a).binary_search(&def.class).is_ok()
                        || !seen.insert((a, def.class))
                    {
                        continue;
                    }
                    if def.complex.iter().all(|&k| has_complex(a, k))
                        && def.atoms.iter().all(|&k| has_atom(a, k))
                    {
                        new_edges.push((a, def.class));
                    }
                }
            }
        }
        if new_edges.is_empty() {
            break;
        }
        new_edges.sort_unstable();
        added += new_edges.len();
        for (a, d) in new_edges {
            let sub = onto.concepts.atomic(a);
            let sup = onto.concepts.atomic(d);
            onto.axioms.push(Axiom::SubClassOf { sub, sup });
        }
    }
    added
}
