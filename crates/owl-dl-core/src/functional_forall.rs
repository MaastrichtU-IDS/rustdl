//! Functional-role closure in definitions (#180): `D ≡ C₁ ⊓ … ⊓ ∃R.B′ ⊓ ∀R.B`
//! with `R` functional and `B′ ⊑ B` ⟹ `C₁ ⊓ … ⊓ ∃R.B′ ⊑ D`.
//!
//! The motivating case is `ore_ont_13859` (SAO): `D ≡ A ⊓ ∃R.B ⊓ ∀R.B`, the
//! usual "closure" pattern, where `R` is functional because its declared inverse
//! is `InverseFunctional`. A class `Y ⊑ A ⊓ ∃R.B` is then `⊑ D`: its one
//! `R`-successor is a `B`, so `∀R.B` holds. The EL saturator cannot read `D`'s
//! sufficient direction because of the `∀`, and the per-pair tableau needs over
//! 120 s for each of the 17 such pairs, so `classify` missed all of them with
//! `incomplete: false`. Emitting the `∀`-free sufficient direction lets the
//! saturator derive them directly.
//!
//! **Rule.** For each definition `D ≡ Y` (`D` atomic, `Y` an `And`), drop every
//! conjunct `∀R.B` for which some other conjunct `∃R.B′` has `B′ ⊑ B`, with `R`
//! functional; if anything was dropped, emit `SubClassOf(⊓ rest, D)`. The `∃`
//! conjuncts always stay in `rest`.
//!
//! **Sound.** `R` functional and `x ∈ ∃R.B′` give `x` exactly one `R`-successor,
//! which is in `B′ ⊆ B`, so `x ∈ ∀R.B`; hence `⊓ rest ⊑ Y ≡ D`. `R` is functional
//! when a signed super-role of it is declared functional:
//! `FunctionalObjectProperty(S)`, or `InverseFunctionalObjectProperty(S)` read
//! as `S⁻` functional, closed down the signed role hierarchy (a sub-role of a
//! functional role is functional). `B′ ⊑ B` is checked structurally only:
//! identity, `B = ⊤`, `B′` a disjunct of `B`, or `B` a conjunct of `B′`. The pass
//! only adds entailed axioms, so it can only add answers.
//!
//! `RUSTDL_FUNCTIONAL_FORALL_DEF` — **default ON**, `=0` disables.

use crate::ir::{ConceptExpr, ConceptId, ConceptPool, Role};
use crate::ontology::{Axiom, InternalOntology};
use std::collections::HashSet;

/// See the module doc. Default ON; `RUSTDL_FUNCTIONAL_FORALL_DEF=0` disables.
#[must_use]
pub fn functional_forall_def_enabled() -> bool {
    std::env::var_os("RUSTDL_FUNCTIONAL_FORALL_DEF").is_none_or(|v| v != "0")
}

/// Every signed role that is functional: declared functional, the inverse of a
/// declared inverse-functional role, or a signed sub-role of either.
fn functional_roles(onto: &InternalOntology) -> HashSet<Role> {
    let declared: Vec<Role> = onto
        .axioms
        .iter()
        .filter_map(|ax| match ax {
            Axiom::FunctionalRole(r) => Some(*r),
            Axiom::InverseFunctionalRole(s) => Some(s.flip()),
            _ => None,
        })
        .collect();
    if declared.is_empty() {
        return HashSet::new();
    }
    let edges = crate::convert::signed_role_edges(onto);
    let mut out = HashSet::new();
    for f in declared {
        out.extend(crate::convert::signed_closure_below(&edges, f));
    }
    out
}

/// `b1 ⊑ b2`, structurally (see the module doc).
fn structurally_sub(b1: ConceptId, b2: ConceptId, pool: &ConceptPool) -> bool {
    if b1 == b2 {
        return true;
    }
    match pool.get(b2) {
        ConceptExpr::Top => return true,
        ConceptExpr::Or(ds) if ds.contains(&b1) => return true,
        _ => {}
    }
    matches!(pool.get(b1), ConceptExpr::And(cs) if cs.contains(&b2))
}

/// The conjuncts of `y` with every `∀R.B` dropped that some `∃R.B′` conjunct
/// implies for a functional `R`; `None` when nothing is dropped or only `∀`
/// conjuncts would remain.
fn implied_free(
    y: ConceptId,
    pool: &ConceptPool,
    functional: &HashSet<Role>,
) -> Option<Vec<ConceptId>> {
    let ConceptExpr::And(cs) = pool.get(y) else {
        return None;
    };
    let exists: Vec<(Role, ConceptId)> = cs
        .iter()
        .filter_map(|&k| match pool.get(k) {
            ConceptExpr::Some(r, b) => Some((*r, *b)),
            _ => None,
        })
        .collect();
    if exists.is_empty() {
        return None;
    }
    let mut rest = Vec::with_capacity(cs.len());
    let mut dropped = false;
    for &k in cs {
        let implied = match pool.get(k) {
            ConceptExpr::All(r, b) => {
                functional.contains(r)
                    && exists
                        .iter()
                        .any(|&(er, eb)| er == *r && structurally_sub(eb, *b, pool))
            }
            _ => false,
        };
        if implied {
            dropped = true;
        } else {
            rest.push(k);
        }
    }
    (dropped && !rest.is_empty()).then_some(rest)
}

/// Emit `SubClassOf(⊓ rest, D)` for every definition the rule applies to.
/// Returns how many axioms were added.
pub fn derive_functional_forall_defs(onto: &mut InternalOntology) -> usize {
    if !functional_forall_def_enabled() {
        return 0;
    }
    let functional = functional_roles(onto);
    if functional.is_empty() {
        return 0;
    }
    let mut todo: Vec<(ConceptId, Vec<ConceptId>)> = Vec::new();
    {
        let pool = &onto.concepts;
        for ax in &onto.axioms {
            let Axiom::EquivalentClasses(ids) = ax else {
                continue;
            };
            for &d in ids {
                if !matches!(pool.get(d), ConceptExpr::Atomic(_)) {
                    continue;
                }
                for &y in ids {
                    if let Some(rest) = implied_free(y, pool, &functional) {
                        todo.push((d, rest));
                    }
                }
            }
        }
    }
    let mut seen: HashSet<(ConceptId, ConceptId)> = HashSet::new();
    let mut added = 0;
    for (d, rest) in todo {
        let lhs = onto.concepts.and(rest);
        if seen.insert((lhs, d)) {
            onto.axioms.push(Axiom::SubClassOf { sub: lhs, sup: d });
            added += 1;
        }
    }
    added
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::convert::convert_ontology;
    use horned_owl::io::ParserConfiguration;
    use horned_owl::io::ofn::reader::read as read_ofn;
    use horned_owl::model::RcStr;
    use horned_owl::ontology::set::SetOntology;
    use std::io::Cursor;

    fn added(body: &str) -> usize {
        let src = format!(
            "Prefix(:=<http://t/>)\nOntology(<http://t/x>\n\
             Declaration(Class(:D)) Declaration(Class(:A)) Declaration(Class(:B))\n\
             Declaration(Class(:C)) Declaration(ObjectProperty(:r)) Declaration(ObjectProperty(:q))\n\
             {body}\n)\n"
        );
        let (onto, _): (SetOntology<RcStr>, _) =
            read_ofn(&mut Cursor::new(src), ParserConfiguration::default()).unwrap();
        // `convert_ontology` already runs the pass; run it again on the result to
        // count what it adds over an axiom set that has no derived axioms yet.
        let mut internal = convert_ontology(&onto).unwrap();
        internal.axioms.retain(|ax| !matches!(ax,
            Axiom::SubClassOf { sub, .. } if matches!(internal.concepts.get(*sub), ConceptExpr::And(_))));
        derive_functional_forall_defs(&mut internal)
    }

    const DEF: &str = "EquivalentClasses(:D ObjectIntersectionOf(:A \
        ObjectSomeValuesFrom(:r :B) ObjectAllValuesFrom(:r :B)))";

    #[test]
    fn fires_on_a_functional_role() {
        assert_eq!(added(&format!("FunctionalObjectProperty(:r) {DEF}")), 1);
    }

    #[test]
    fn fires_through_an_inverse_functional_inverse() {
        assert_eq!(
            added(&format!(
                "InverseObjectProperties(:q :r) InverseFunctionalObjectProperty(:q) {DEF}"
            )),
            1
        );
    }

    /// FP guard: without functionality the `∀` is not implied.
    #[test]
    fn does_not_fire_without_functionality() {
        assert_eq!(added(DEF), 0);
    }

    /// FP guard: `InverseFunctional(r)` makes `r⁻`, not `r`, functional.
    #[test]
    fn inverse_functional_on_the_role_itself_does_not_count() {
        assert_eq!(
            added(&format!("InverseFunctionalObjectProperty(:r) {DEF}")),
            0
        );
    }

    /// FP guard: the `∃` filler must be inside the `∀` filler.
    #[test]
    fn does_not_fire_when_the_fillers_do_not_match() {
        assert_eq!(
            added(
                "FunctionalObjectProperty(:r) EquivalentClasses(:D ObjectIntersectionOf(:A \
                 ObjectSomeValuesFrom(:r :C) ObjectAllValuesFrom(:r :B)))"
            ),
            0
        );
    }

    #[test]
    fn a_disjunct_filler_is_inside_the_union() {
        assert_eq!(
            added(
                "FunctionalObjectProperty(:r) EquivalentClasses(:D ObjectIntersectionOf(:A \
                 ObjectSomeValuesFrom(:r :B) ObjectAllValuesFrom(:r ObjectUnionOf(:B :C))))"
            ),
            1
        );
    }
}
