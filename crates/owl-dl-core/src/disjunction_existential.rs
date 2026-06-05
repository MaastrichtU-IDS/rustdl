//! Preprocessing pass: derive `X ⊑ ∃R.C` from
//! `X ⊑ ∃R.(D₁ ⊔ … ⊔ Dₙ)` when all disjuncts share a told-subsumer `C`.
//!
//! ## Why
//!
//! The consequence-based EL saturator drops existentials whose filler is
//! a disjunction (`∃R.(D₁ ⊔ … ⊔ Dₙ)` is out of EL). But when every
//! disjunct shares a common subsumer `C` — i.e. `Dᵢ ⊑ C` for all `i` —
//! the disjunction is eliminable by cases: `(D₁ ⊔ … ⊔ Dₙ) ⊑ C`, hence
//! `∃R.(D₁ ⊔ … ⊔ Dₙ) ⊑ ∃R.C`. Feeding the saturator the derived
//! `X ⊑ ∃R.C` lets it close subsumptions that otherwise need a full
//! tableau case-split.
//!
//! This is a **sound under-approximation**: every emitted axiom is
//! entailed, and we only use *told* (explicit, transitively-closed)
//! subsumers of *atomic* disjuncts, so no false positive is possible.
//! Cases where the common subsumer is only *derived* (not told), or a
//! disjunct is non-atomic, are left to the tableau/wedge.
//!
//! ## Impact
//!
//! Closes the SIO corpus MISSES `SIO_010092 ⊑ SIO_001353` and
//! `SIO_010092 ⊑ SIO_010410`: `SIO_010092` (DNA template) is
//! `⊑ ∃has-function.(template-for-RNA ⊔ template-for-DNA)`, both
//! disjuncts `⊑` `SIO_010088` (template-for-molecular-synthesis)
//! `⊑ realizable-entity`, and `has-function ⊑* has-realizable-property`.

use crate::ir::{ClassId, ConceptExpr, ConceptId, ConceptPool, Role};
use crate::ontology::{Axiom, InternalOntology};
use crate::told::{ToldTables, build_told_tables};

/// Scan `onto` for `SubClassOf(X, ∃R.(union-of-atomics))` (directly, or
/// as a conjunct of a top-level `And`) and append a derived
/// `SubClassOf(X, ∃R.C)` for each *minimal* common told-subsumer `C` of
/// the disjuncts. See the module docs for soundness.
pub fn derive_disjunction_existentials(onto: &mut InternalOntology) {
    let told = build_told_tables(onto);
    // Phase 1 (immutable borrow): collect (sub, role, common-class).
    let mut triples: Vec<(ConceptId, Role, ClassId)> = Vec::new();
    for ax in &onto.axioms {
        let Axiom::SubClassOf { sub, sup } = ax else {
            continue;
        };
        collect_from_sup(*sub, *sup, &onto.concepts, &told, &mut triples);
    }
    if triples.is_empty() {
        return;
    }
    // Phase 2 (mutable borrow): intern the derived existentials + push.
    for (sub, role, c) in triples {
        let body = onto.concepts.atomic(c);
        let sup = onto.concepts.some(role, body);
        if sub == sup {
            continue;
        }
        onto.axioms.push(Axiom::SubClassOf { sub, sup });
    }
}

/// Handle a single `SubClassOf` super-concept: a direct `∃R.(union)` or
/// each `∃R.(union)` conjunct of a top-level `And`.
fn collect_from_sup(
    sub: ConceptId,
    sup: ConceptId,
    pool: &ConceptPool,
    told: &ToldTables,
    out: &mut Vec<(ConceptId, Role, ClassId)>,
) {
    match pool.get(sup) {
        ConceptExpr::Some(role, body) => {
            for c in minimal_common_subsumers(*body, pool, told) {
                out.push((sub, *role, c));
            }
        }
        ConceptExpr::And(operands) => {
            for &op in operands {
                if let ConceptExpr::Some(role, body) = pool.get(op) {
                    for c in minimal_common_subsumers(*body, pool, told) {
                        out.push((sub, *role, c));
                    }
                }
            }
        }
        _ => {}
    }
}

/// If `body` is `Or(D₁, …, Dₙ)` with all `Dᵢ` atomic and `n ≥ 2`,
/// return the *minimal* (most specific) classes `C` such that every
/// `Dᵢ ⊑ C` is told. Empty otherwise.
fn minimal_common_subsumers(
    body: ConceptId,
    pool: &ConceptPool,
    told: &ToldTables,
) -> Vec<ClassId> {
    let ConceptExpr::Or(disjuncts) = pool.get(body) else {
        return Vec::new();
    };
    let mut atoms: Vec<ClassId> = Vec::with_capacity(disjuncts.len());
    for &d in disjuncts {
        match pool.get(d) {
            ConceptExpr::Atomic(c) => atoms.push(*c),
            // A non-atomic disjunct (nested ∃, And, …) is left to the
            // tableau — keep this pass a sound under-approximation.
            _ => return Vec::new(),
        }
    }
    if atoms.len() < 2 {
        return Vec::new();
    }
    // Intersection of the (reflexive, transitively-closed, sorted)
    // told-super-class sets. Reflexivity is sound here: a disjunct `Dᵢ`
    // lands in the intersection only if it told-subsumes every other
    // disjunct, in which case `Dᵢ ⊒ (D₁ ⊔ … ⊔ Dₙ)` and `∃R.Dᵢ` holds.
    let mut common: Vec<ClassId> = told.super_classes(atoms[0]).to_vec();
    for &a in &atoms[1..] {
        let supers = told.super_classes(a);
        common.retain(|c| supers.binary_search(c).is_ok());
        if common.is_empty() {
            return Vec::new();
        }
    }
    // Keep only minimal elements: drop `C` if some other common `C'` is
    // told-below `C` (the saturator recovers the weaker supers from the
    // minimal ones, so emitting the whole chain is redundant).
    common
        .iter()
        .copied()
        .filter(|&c| {
            !common
                .iter()
                .any(|&other| other != c && told.is_told_sub(other, c))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::ir::{ConceptExpr, Role};
    use horned_owl::io::ParserConfiguration;
    use horned_owl::io::ofn::reader::read;
    use horned_owl::model::RcStr;
    use horned_owl::ontology::set::SetOntology;
    use std::io::Cursor;

    /// The SIO pattern: `X ⊑ ∃R.(D1 ⊔ D2)` with `D1,D2 ⊑ E ⊑ F`. The
    /// pass (run inside `convert_ontology`) must add `X ⊑ ∃R.E` (E is
    /// the minimal common told-subsumer), and must NOT add the weaker
    /// `X ⊑ ∃R.F` (only minimal subsumers). Mirrors `SIO_010092`'s
    /// `∃has-function.(template-RNA ⊔ template-DNA)`.
    #[test]
    fn pass_emits_minimal_common_subsumer_existential() {
        let src = "\
Prefix(:=<http://t.org/#>)
Ontology(
  Declaration(Class(:X)) Declaration(Class(:D1)) Declaration(Class(:D2))
  Declaration(Class(:E)) Declaration(Class(:F))
  Declaration(ObjectProperty(:R))
  SubClassOf(:X ObjectSomeValuesFrom(:R ObjectUnionOf(:D1 :D2)))
  SubClassOf(:D1 :E) SubClassOf(:D2 :E) SubClassOf(:E :F)
)
";
        let (set_onto, _): (SetOntology<RcStr>, _) =
            read(&mut Cursor::new(src), ParserConfiguration::default()).expect("parses");
        // convert_ontology runs the pass.
        let onto = crate::convert::convert_ontology(&set_onto).expect("converts");
        let cid = |iri: &str| onto.vocabulary.class_id(iri).expect("declared");
        let x = cid("http://t.org/#X");
        let e = cid("http://t.org/#E");
        let f = cid("http://t.org/#F");

        let some_class = |target| {
            onto.axioms.iter().any(|ax| {
                if let crate::ontology::Axiom::SubClassOf { sub, sup } = ax {
                    matches!(onto.concepts.get(*sub), ConceptExpr::Atomic(c) if *c == x)
                        && matches!(onto.concepts.get(*sup),
                            ConceptExpr::Some(Role::Named(_), body)
                                if matches!(onto.concepts.get(*body), ConceptExpr::Atomic(c) if *c == target))
                } else {
                    false
                }
            })
        };
        assert!(
            some_class(e),
            "expected derived X ⊑ ∃R.E (minimal common subsumer)"
        );
        assert!(
            !some_class(f),
            "should NOT emit the non-minimal X ⊑ ∃R.F (E ⊑ F already covers it)"
        );
    }

    /// No common subsumer ⇒ no derived axiom (and no panic).
    #[test]
    fn pass_no_common_subsumer_emits_nothing() {
        let src = "\
Prefix(:=<http://t.org/#>)
Ontology(
  Declaration(Class(:X)) Declaration(Class(:D1)) Declaration(Class(:D2))
  Declaration(ObjectProperty(:R))
  SubClassOf(:X ObjectSomeValuesFrom(:R ObjectUnionOf(:D1 :D2)))
)
";
        let (set_onto, _): (SetOntology<RcStr>, _) =
            read(&mut Cursor::new(src), ParserConfiguration::default()).expect("parses");
        let onto = crate::convert::convert_ontology(&set_onto).expect("converts");
        let x = onto.vocabulary.class_id("http://t.org/#X").expect("X");
        // The only ∃R.* axiom on X is the original union; no atomic-body
        // existential was derived (D1, D2 share no told subsumer).
        let derived = onto.axioms.iter().any(|ax| {
            matches!(ax, crate::ontology::Axiom::SubClassOf { sub, sup }
                if matches!(onto.concepts.get(*sub), ConceptExpr::Atomic(c) if *c == x)
                    && matches!(onto.concepts.get(*sup),
                        ConceptExpr::Some(_, body) if matches!(onto.concepts.get(*body), ConceptExpr::Atomic(_))))
        });
        assert!(!derived, "no common subsumer ⇒ nothing derived");
    }
}
