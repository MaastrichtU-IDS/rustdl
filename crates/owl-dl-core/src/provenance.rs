//! #586: which source component a stored IR axiom came from, so a surface that cites
//! "the source axioms" (`prove --json`) can cite them as written.
//!
//! [`crate::convert::convert_ontology`] does not keep the source's form. It normalises
//! each component (`ObjectHasValue` becomes `∃r.{a}`, `owl:Thing` becomes ⊤, an exact
//! cardinality becomes `≥n ⊓ ≤n`, `InverseFunctional(r⁻)` becomes `Functional(r)`,
//! repeated n-ary operands collapse) and later passes rewrite some axioms in place
//! (a union on the left is split; `X ⊑ ¬Y` becomes `X ⊓ Y ⊑ ⊥`) or replace them
//! (chains longer than 2 become pieces through aux roles). Reverse-rendering the stored
//! axiom therefore often yields an axiom that is not in the source.
//!
//! This module rebuilds the map after the fact. It converts every source component
//! again, against a scratch copy of the converted ontology's vocabulary and concept pool,
//! so ids match. It replays the per-axiom rewrites on the result and records each IR form
//! the component can take. Conversion itself is untouched. An IR axiom no component
//! produces is a DERIVED axiom (a later pass built it from several axioms or from the OWL
//! semantics) and has no single source.
//!
//! A hit is never a fabricated citation: the cited component is in the source, and it
//! converted to exactly that IR axiom (for a chain piece, to the chain the piece is part
//! of: the pieces are a conservative extension of the chain through their aux role, not
//! entailed by it alone). Where several components give the same IR axiom (salvage and
//! the union split make that possible for non-equivalent ones), the cited one entails the
//! IR axiom but need not be the weakest such component.

use std::collections::HashMap;

use horned_owl::model::{AnnotatedComponent, ForIRI};
use horned_owl::ontology::set::SetOntology;

use crate::convert::{
    convert_component, salvage_dropped_conjuncts_enabled, salvage_necessary_conjuncts,
};
use crate::ir::Role;
use crate::ontology::{Axiom, InternalOntology, SubRolePath};

/// The aux-role IRI prefix [`crate::convert`]'s chain decomposition uses
/// (`urn:rustdl-aux-role:<axiom-idx>:<leg-idx>`).
const AUX_ROLE_PREFIX: &str = "urn:rustdl-aux-role:";

/// Every IR form a source component of `src` takes in `internal`, mapped to that
/// component. When several components give the same IR axiom, the least one in the
/// components' order wins, so the result does not
/// depend on the set's iteration order.
///
/// `internal` must be `convert_ontology(src)` (in this process, so the same `RUSTDL_*`
/// flags apply). One extra conversion pass over `src`; meant for a single `prove`
/// query, not for a hot path.
#[must_use]
pub fn literal_sources<'a, A: ForIRI>(
    src: &'a SetOntology<A>,
    internal: &InternalOntology,
) -> HashMap<Axiom, &'a AnnotatedComponent<A>> {
    // One scratch ontology for all components: interning is by IRI and hash-consed, so
    // ids already in `internal` are reproduced, and anything new gets a fresh id that no
    // stored axiom uses.
    let mut scratch = InternalOntology::new();
    scratch.vocabulary = internal.vocabulary.clone();
    scratch.concepts = internal.concepts.clone();
    let salvage = salvage_dropped_conjuncts_enabled();
    let mut map: HashMap<Axiom, &AnnotatedComponent<A>> = HashMap::new();
    // Unsorted iteration, keeping the least component on a collision: the same result as
    // a sorted pass without a second sort of every component.
    for ac in src {
        for ax in stored_forms(ac, &mut scratch, salvage) {
            map.entry(ax)
                .and_modify(|cur| {
                    if ac < *cur {
                        *cur = ac;
                    }
                })
                .or_insert(ac);
        }
    }
    map
}

/// The IR axioms one component becomes: its conversion (or, when conversion rejects it,
/// the salvaged conjuncts), plus the forms the in-place rewrites of `convert_ontology`
/// give them. Every intermediate form is kept: a rewrite that is switched off leaves the
/// earlier form stored.
fn stored_forms<A: ForIRI>(
    ac: &AnnotatedComponent<A>,
    scratch: &mut InternalOntology,
    salvage: bool,
) -> Vec<Axiom> {
    let canonical = crate::set_operands::canonical(&ac.component);
    let component = canonical.as_ref().unwrap_or(&ac.component);
    scratch.axioms.clear();
    match convert_component(component, &mut scratch.vocabulary, &mut scratch.concepts) {
        Ok(Some(ax)) => scratch.axioms.push(ax),
        Ok(None) => return Vec::new(),
        Err(_) => {
            if !salvage {
                return Vec::new();
            }
            let kept = salvage_necessary_conjuncts(
                component,
                &mut scratch.vocabulary,
                &mut scratch.concepts,
            );
            scratch.axioms.extend(kept);
        }
    }
    let mut forms = scratch.axioms.clone();
    // The same order as `convert_ontology`: the union split, then the negation rewrite.
    crate::disjunctive_antecedent::split_disjunctive_antecedents(scratch);
    forms.extend(scratch.axioms.iter().cloned());
    let _ = crate::negation_gci::rewrite_negated_supers(scratch);
    forms.append(&mut scratch.axioms);
    forms
}

/// If `piece` is one of the axioms chain decomposition made from a chain longer than 2
/// (`R₁∘R₂ ⊑ aux₁`, `aux₁∘R₃ ⊑ aux₂`, …, `auxₙ∘Rₙ₊₁ ⊑ S`), the original
/// `R₁∘…∘Rₙ₊₁ ⊑ S`, rebuilt from the decomposition's pieces in `internal`. `None` when
/// `piece` names no aux role, or when the pieces sharing its decomposition site do not
/// form exactly one linear cascade (e.g. a user role in the reserved namespace joined
/// it): the caller then treats `piece` as derived.
#[must_use]
pub fn decomposed_chain_origin(internal: &InternalOntology, piece: &Axiom) -> Option<Axiom> {
    let vocab = &internal.vocabulary;
    // The decomposition site of an aux role: the `<axiom-idx>` of its IRI.
    let site = |r: Role| -> Option<&str> {
        if r.is_inverse() {
            return None;
        }
        let rest = vocab.role_iri(r.role_id()).strip_prefix(AUX_ROLE_PREFIX)?;
        rest.split_once(':').map(|(idx, _)| idx)
    };
    let two_chain = |ax: &Axiom| -> Option<(Role, Role, Role)> {
        match ax {
            Axiom::SubObjectPropertyOf {
                sub: SubRolePath::Chain(parts),
                sup,
            } if parts.len() == 2 => Some((parts[0], parts[1], *sup)),
            _ => None,
        }
    };
    let (a, _, sup) = two_chain(piece)?;
    let idx = site(a).or_else(|| site(sup))?;
    // Every piece of this site: a 2-chain whose first leg or super-role is one of its
    // aux roles.
    let pieces: Vec<(Role, Role, Role)> = internal
        .axioms
        .iter()
        .filter_map(two_chain)
        .filter(|&(x, _, s)| site(x) == Some(idx) || site(s) == Some(idx))
        .collect();
    // The head: the one piece whose first leg is not an aux role of this site.
    let mut heads = pieces.iter().filter(|&&(x, _, _)| site(x) != Some(idx));
    let &(first, second, mut cur) = heads.next()?;
    if heads.next().is_some() || site(cur) != Some(idx) {
        return None;
    }
    let mut parts = vec![first, second];
    let mut used = 1;
    // Follow the cascade: exactly one consumer per aux role.
    while site(cur) == Some(idx) {
        let mut consumers = pieces.iter().filter(|&&(x, _, _)| x == cur);
        let &(_, leg, next) = consumers.next()?;
        if consumers.next().is_some() {
            return None;
        }
        parts.push(leg);
        cur = next;
        used += 1;
    }
    if used != pieces.len() {
        return None;
    }
    Some(Axiom::SubObjectPropertyOf {
        sub: SubRolePath::Chain(parts),
        sup: cur,
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use crate::convert::convert_ontology;
    use horned_owl::io::ParserConfiguration;
    use horned_owl::io::ofn::reader::read;
    use horned_owl::model::{Component, RcStr};
    use std::io::Cursor;

    fn parse(body: &str) -> SetOntology<RcStr> {
        let src = format!(
            "Prefix(:=<http://t/>)\nPrefix(owl:=<http://www.w3.org/2002/07/owl#>)\n\
             Ontology(<http://t/o>\n{body}\n)\n"
        );
        read(&mut Cursor::new(src), ParserConfiguration::default())
            .unwrap()
            .0
    }

    fn is_piece(ax: &Axiom) -> bool {
        matches!(ax, Axiom::SubObjectPropertyOf { sub: SubRolePath::Chain(p), .. } if p.len() == 2)
    }

    #[test]
    fn every_piece_of_a_four_chain_leads_back_to_its_source() {
        let src = parse(
            "SubObjectPropertyOf(ObjectPropertyChain(:p :q :s :u) :t)\n\
             SubObjectPropertyOf(ObjectPropertyChain(:q :s :u) :v)",
        );
        let internal = convert_ontology(&src).unwrap();
        let sources = literal_sources(&src, &internal);
        let pieces: Vec<&Axiom> = internal.axioms.iter().filter(|a| is_piece(a)).collect();
        assert_eq!(pieces.len(), 3 + 2, "{:?}", internal.axioms);
        let mut origins = BTreeSet::new();
        for piece in pieces {
            let chain = decomposed_chain_origin(&internal, piece).unwrap();
            let Component::SubObjectPropertyOf(spo) = &sources.get(&chain).unwrap().component
            else {
                panic!("{chain:?}");
            };
            origins.insert(format!("{:?}", spo.sup));
        }
        // Each piece leads to its own chain, never to the other one.
        assert_eq!(origins.len(), 2, "{origins:?}");
    }

    #[test]
    fn a_user_role_in_the_aux_namespace_breaks_the_cascade() {
        // The decomposition of the 3-chain (at axiom index 0) uses
        // `urn:rustdl-aux-role:0:1`. A user axiom producing that same role gives it two
        // producers, so the pieces no longer form one cascade.
        let src = parse(
            "SubObjectPropertyOf(ObjectPropertyChain(:p :q :s) :t)\n\
             SubObjectPropertyOf(ObjectPropertyChain(:a :b) <urn:rustdl-aux-role:0:1>)",
        );
        let internal = convert_ontology(&src).unwrap();
        let aux_consumer = internal
            .axioms
            .iter()
            .find(|a| {
                matches!(a, Axiom::SubObjectPropertyOf { sub: SubRolePath::Chain(p), .. }
                    if p.len() == 2
                        && internal.vocabulary.role_iri(p[0].role_id()).starts_with(AUX_ROLE_PREFIX))
            })
            .unwrap();
        assert_eq!(decomposed_chain_origin(&internal, aux_consumer), None);
    }

    #[test]
    fn a_normalised_component_maps_to_itself_and_a_derived_axiom_to_nothing() {
        let src = parse(
            "Declaration(NamedIndividual(:a))\n\
             SubClassOf(:X ObjectHasValue(:r :a))\n\
             SubClassOf(:X ObjectComplementOf(:Y))",
        );
        let internal = convert_ontology(&src).unwrap();
        let sources = literal_sources(&src, &internal);
        let mut cited = BTreeSet::new();
        for ax in &internal.axioms {
            if let Some(ac) = sources.get(ax) {
                cited.insert(format!("{:?}", ac.component));
            }
        }
        assert!(
            cited.iter().any(|c| c.contains("ObjectHasValue")),
            "{cited:?}"
        );
        assert!(
            cited.iter().any(|c| c.contains("ObjectComplementOf")),
            "{cited:?}"
        );
        // An axiom built only here, which no component converts to, has no source.
        let mut scratch = internal.clone();
        let x = scratch
            .concepts
            .atomic(scratch.vocabulary.intern_class("http://t/X"));
        let nobody = scratch
            .concepts
            .atomic(scratch.vocabulary.intern_class("http://t/Nobody"));
        assert!(!sources.contains_key(&Axiom::SubClassOf {
            sub: x,
            sup: nobody
        }));
    }

    use std::collections::BTreeSet;
}
