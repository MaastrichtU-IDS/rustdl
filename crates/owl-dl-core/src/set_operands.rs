//! #538: the operands of OWL 2's n-ary axioms are a SET.
//!
//! The OWL 2 Structural Specification (§2.1): "By default, all associations are sets",
//! and duplicates in an exchange syntax "SHOULD be eliminated" when a document is read.
//! horned-owl keeps the operands of `DifferentIndividuals`, `DisjointClasses`,
//! `DisjointUnion`, … as a `Vec`, so `DifferentIndividuals(:a :a)` arrived as the list
//! `[a, a]` and was read as `a ≠ a`: a false inconsistency. [`canonical`] removes every
//! operand structurally equal to an earlier one, so every reader sees the set.
//!
//! **One definition for every reader.** [`convert_ontology`](crate::convert::convert_ontology)
//! canonicalises each component before it sorts and converts them, and every module that
//! reads these axiom kinds from the raw horned-owl ontology iterates [`components`]
//! instead of the ontology itself. A reader that bypasses both reads the list again.
//!
//! **Structural equality** is the specification's: two operands are equal when they are
//! the same expression, where the operands of a nested set-valued constructor
//! (`ObjectIntersectionOf`, `ObjectUnionOf`, `ObjectOneOf`, `DataIntersectionOf`,
//! `DataUnionOf`, `DataOneOf`, a datatype restriction's facets) compare as sets. Nothing
//! else is identified: `"1"^^xsd:integer` and `"01"^^xsd:integer`, `A` and `¬¬A`, or `A`
//! and `A ⊓ owl:Thing` are different operands, though they denote the same thing, so
//! `DisjointClasses(:A ObjectIntersectionOf(:A owl:Thing))` still makes `A` unsat.
//!
//! **Why this is the safe direction under either reading.** Collapsing only ever removes
//! a constraint (`Disjoint*`, `DifferentIndividuals`) or a repetition that adds nothing
//! (`Equivalent*`, `SameIndividual`, `HasKey`, `DisjointUnion`'s covering). Under the set
//! reading the answers derived from duplicates were false positives; under a list
//! reading, collapsing loses positives, never adds one, and
//! [`InternalOntology::collapsed_operands`](crate::InternalOntology::collapsed_operands)
//! makes it visible.
//!
//! [`canonical`] keeps the axiom kind. An axiom left with ONE operand is the caller's to
//! interpret: `DisjointClasses(C)` / `DifferentIndividuals(a)` / `Equivalent*(x)` say
//! nothing, while `DisjointUnion(D, C)` still says `D ≡ C`.
use std::borrow::Cow;
use std::collections::HashSet;
use std::hash::Hash;

use horned_owl::model::{
    AnnotatedComponent, ClassExpression as CE, Component as C, DataRange as DR,
    DifferentIndividuals, DisjointClasses, DisjointDataProperties, DisjointObjectProperties,
    DisjointUnion, EquivalentClasses, EquivalentDataProperties, EquivalentObjectProperties, ForIRI,
    HasKey, SameIndividual,
};
use horned_owl::ontology::set::SetOntology;

/// Above this many operands, dedup hashes canonical keys instead of comparing pairwise.
const PAIRWISE_MAX: usize = 16;

/// How many axioms [`canonical`] changed while an ontology was read (#538).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CollapsedOperands {
    /// Axioms that had a repeated operand.
    pub axioms: u64,
    /// Of those, the axioms left with ONE operand, which therefore say nothing (a
    /// one-member `DisjointUnion`, which still says `D ≡ C`, is not counted here).
    pub vacuous: u64,
}

impl CollapsedOperands {
    /// Count one canonicalised component.
    pub fn record<A: ForIRI>(&mut self, canonical: &C<A>) {
        self.axioms += 1;
        if operand_count(canonical).is_some_and(|n| n < 2)
            && !matches!(canonical, C::DisjointUnion(_))
        {
            self.vacuous += 1;
        }
    }

    /// Whether no axiom was changed.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.axioms == 0
    }
}

/// The number of set-valued operands of `c`, or `None` when `c` has none.
#[must_use]
pub fn operand_count<A: ForIRI>(c: &C<A>) -> Option<usize> {
    Some(match c {
        C::DifferentIndividuals(ax) => ax.0.len(),
        C::SameIndividual(ax) => ax.0.len(),
        C::DisjointClasses(ax) => ax.0.len(),
        C::EquivalentClasses(ax) => ax.0.len(),
        C::DisjointUnion(ax) => ax.1.len(),
        C::DisjointObjectProperties(ax) => ax.0.len(),
        C::EquivalentObjectProperties(ax) => ax.0.len(),
        C::DisjointDataProperties(ax) => ax.0.len(),
        C::EquivalentDataProperties(ax) => ax.0.len(),
        C::HasKey(ax) => ax.vpe.len(),
        _ => return None,
    })
}

/// `c` with every operand structurally equal to an earlier one removed, keeping the
/// first occurrence of each and the axiom kind; `None` when no operand repeats (the
/// common case, which allocates nothing).
#[must_use]
pub fn canonical<A: ForIRI>(c: &C<A>) -> Option<C<A>> {
    Some(match c {
        C::DifferentIndividuals(ax) => C::DifferentIndividuals(DifferentIndividuals(dedup(
            &ax.0,
            PartialEq::eq,
            Cow::Borrowed,
        )?)),
        C::SameIndividual(ax) => {
            C::SameIndividual(SameIndividual(dedup(&ax.0, PartialEq::eq, Cow::Borrowed)?))
        }
        C::DisjointClasses(ax) => C::DisjointClasses(DisjointClasses(dedup(&ax.0, ce_eq, ce_key)?)),
        C::EquivalentClasses(ax) => {
            C::EquivalentClasses(EquivalentClasses(dedup(&ax.0, ce_eq, ce_key)?))
        }
        C::DisjointUnion(ax) => {
            C::DisjointUnion(DisjointUnion(ax.0.clone(), dedup(&ax.1, ce_eq, ce_key)?))
        }
        C::DisjointObjectProperties(ax) => C::DisjointObjectProperties(DisjointObjectProperties(
            dedup(&ax.0, PartialEq::eq, Cow::Borrowed)?,
        )),
        C::EquivalentObjectProperties(ax) => C::EquivalentObjectProperties(
            EquivalentObjectProperties(dedup(&ax.0, PartialEq::eq, Cow::Borrowed)?),
        ),
        C::DisjointDataProperties(ax) => C::DisjointDataProperties(DisjointDataProperties(dedup(
            &ax.0,
            PartialEq::eq,
            Cow::Borrowed,
        )?)),
        C::EquivalentDataProperties(ax) => C::EquivalentDataProperties(EquivalentDataProperties(
            dedup(&ax.0, PartialEq::eq, Cow::Borrowed)?,
        )),
        C::HasKey(ax) => C::HasKey(HasKey {
            ce: ax.ce.clone(),
            vpe: dedup(&ax.vpe, PartialEq::eq, Cow::Borrowed)?,
        }),
        _ => return None,
    })
}

/// [`canonical`] on an annotated component, keeping its annotations.
#[must_use]
pub fn canonical_annotated<A: ForIRI>(
    ac: &AnnotatedComponent<A>,
) -> Cow<'_, AnnotatedComponent<A>> {
    match canonical(&ac.component) {
        None => Cow::Borrowed(ac),
        Some(component) => Cow::Owned(AnnotatedComponent {
            component,
            ann: ac.ann.clone(),
        }),
    }
}

/// Every component of `onto`, canonicalised: how a module that reads the raw horned-owl
/// ontology sees the set reading (#538).
pub fn components<A: ForIRI>(
    onto: &SetOntology<A>,
) -> impl Iterator<Item = Cow<'_, AnnotatedComponent<A>>> {
    onto.iter().map(canonical_annotated)
}

/// The number of distinct elements of a set-valued operand list: `ObjectOneOf(:a :a)`
/// has one member. Plain equality, which is structural for individuals and literals.
#[must_use]
pub fn distinct_len<T: PartialEq>(xs: &[T]) -> usize {
    xs.iter()
        .enumerate()
        .filter(|&(j, x)| !xs[..j].contains(x))
        .count()
}

/// What [`convert_ontology`](crate::convert::convert_ontology) records in
/// `collapsed_operands` for `onto`, without converting it.
#[must_use]
pub fn count<A: ForIRI>(onto: &SetOntology<A>) -> CollapsedOperands {
    let mut out = CollapsedOperands::default();
    for ac in onto {
        if let Some(c) = canonical(&ac.component) {
            out.record(&c);
        }
    }
    out
}

/// The operands without those equal to an earlier one, or `None` when none repeats.
/// `eq` is structural equality; `key` maps an operand to a canonical form whose plain
/// equality is `eq` (used to hash long operand lists).
fn dedup<'a, T: Clone + Eq + Hash>(
    xs: &'a [T],
    eq: impl Fn(&T, &T) -> bool,
    key: impl Fn(&'a T) -> Cow<'a, T>,
) -> Option<Vec<T>> {
    if xs.len() <= PAIRWISE_MAX {
        let first = (1..xs.len()).find(|&j| xs[..j].iter().any(|x| eq(x, &xs[j])))?;
        let mut out: Vec<T> = xs[..first].to_vec();
        for x in &xs[first + 1..] {
            if !out.iter().any(|y| eq(y, x)) {
                out.push(x.clone());
            }
        }
        return Some(out);
    }
    let keys: Vec<Cow<'a, T>> = xs.iter().map(key).collect();
    let mut seen: HashSet<&Cow<'a, T>> = HashSet::with_capacity(keys.len());
    let keep: Vec<bool> = keys.iter().map(|k| seen.insert(k)).collect();
    if keep.iter().all(|&k| k) {
        return None;
    }
    Some(
        xs.iter()
            .zip(keep)
            .filter(|&(_, k)| k)
            .map(|(x, _)| x.clone())
            .collect(),
    )
}

/// Structural equality of class expressions: nested set-valued operands compare as sets.
/// Allocates nothing.
#[must_use]
pub fn ce_eq<A: ForIRI>(a: &CE<A>, b: &CE<A>) -> bool {
    match (a, b) {
        (CE::ObjectIntersectionOf(xs), CE::ObjectIntersectionOf(ys))
        | (CE::ObjectUnionOf(xs), CE::ObjectUnionOf(ys)) => same_set(xs, ys, ce_eq),
        (CE::ObjectOneOf(xs), CE::ObjectOneOf(ys)) => same_set(xs, ys, PartialEq::eq),
        (CE::ObjectComplementOf(x), CE::ObjectComplementOf(y)) => ce_eq(x, y),
        (
            CE::ObjectSomeValuesFrom { ope: p, bce: x },
            CE::ObjectSomeValuesFrom { ope: q, bce: y },
        )
        | (
            CE::ObjectAllValuesFrom { ope: p, bce: x },
            CE::ObjectAllValuesFrom { ope: q, bce: y },
        ) => p == q && ce_eq(x, y),
        (
            CE::ObjectMinCardinality { n, ope: p, bce: x },
            CE::ObjectMinCardinality {
                n: m,
                ope: q,
                bce: y,
            },
        )
        | (
            CE::ObjectMaxCardinality { n, ope: p, bce: x },
            CE::ObjectMaxCardinality {
                n: m,
                ope: q,
                bce: y,
            },
        )
        | (
            CE::ObjectExactCardinality { n, ope: p, bce: x },
            CE::ObjectExactCardinality {
                n: m,
                ope: q,
                bce: y,
            },
        ) => n == m && p == q && ce_eq(x, y),
        (CE::DataSomeValuesFrom { dp: p, dr: x }, CE::DataSomeValuesFrom { dp: q, dr: y })
        | (CE::DataAllValuesFrom { dp: p, dr: x }, CE::DataAllValuesFrom { dp: q, dr: y }) => {
            p == q && dr_eq(x, y)
        }
        (
            CE::DataMinCardinality { n, dp: p, dr: x },
            CE::DataMinCardinality { n: m, dp: q, dr: y },
        )
        | (
            CE::DataMaxCardinality { n, dp: p, dr: x },
            CE::DataMaxCardinality { n: m, dp: q, dr: y },
        )
        | (
            CE::DataExactCardinality { n, dp: p, dr: x },
            CE::DataExactCardinality { n: m, dp: q, dr: y },
        ) => n == m && p == q && dr_eq(x, y),
        // Every other pair: plain equality (a different discriminant is unequal at once).
        _ => a == b,
    }
}

/// [`ce_eq`] for data ranges.
#[must_use]
pub fn dr_eq<A: ForIRI>(a: &DR<A>, b: &DR<A>) -> bool {
    match (a, b) {
        (DR::DataIntersectionOf(xs), DR::DataIntersectionOf(ys))
        | (DR::DataUnionOf(xs), DR::DataUnionOf(ys)) => same_set(xs, ys, dr_eq),
        (DR::DataOneOf(xs), DR::DataOneOf(ys)) => same_set(xs, ys, PartialEq::eq),
        (DR::DataComplementOf(x), DR::DataComplementOf(y)) => dr_eq(x, y),
        (DR::DatatypeRestriction(s, xs), DR::DatatypeRestriction(t, ys)) => {
            s == t && same_set(xs, ys, PartialEq::eq)
        }
        _ => a == b,
    }
}

/// Whether every element of each list equals some element of the other.
fn same_set<T>(xs: &[T], ys: &[T], eq: impl Fn(&T, &T) -> bool) -> bool {
    xs.iter().all(|x| ys.iter().any(|y| eq(x, y))) && ys.iter().all(|y| xs.iter().any(|x| eq(x, y)))
}

/// A canonical form of `c` whose plain equality is [`ce_eq`]: borrowed when `c` contains
/// no set-valued constructor (then the two coincide).
fn ce_key<A: ForIRI>(c: &CE<A>) -> Cow<'_, CE<A>> {
    if ce_has_set(c) {
        Cow::Owned(ce_canon(c))
    } else {
        Cow::Borrowed(c)
    }
}

fn ce_has_set<A: ForIRI>(c: &CE<A>) -> bool {
    match c {
        CE::ObjectIntersectionOf(_) | CE::ObjectUnionOf(_) | CE::ObjectOneOf(_) => true,
        CE::ObjectComplementOf(x)
        | CE::ObjectSomeValuesFrom { bce: x, .. }
        | CE::ObjectAllValuesFrom { bce: x, .. }
        | CE::ObjectMinCardinality { bce: x, .. }
        | CE::ObjectMaxCardinality { bce: x, .. }
        | CE::ObjectExactCardinality { bce: x, .. } => ce_has_set(x),
        CE::DataSomeValuesFrom { dr, .. }
        | CE::DataAllValuesFrom { dr, .. }
        | CE::DataMinCardinality { dr, .. }
        | CE::DataMaxCardinality { dr, .. }
        | CE::DataExactCardinality { dr, .. } => dr_has_set(dr),
        _ => false,
    }
}

fn dr_has_set<A: ForIRI>(d: &DR<A>) -> bool {
    match d {
        DR::DataIntersectionOf(_)
        | DR::DataUnionOf(_)
        | DR::DataOneOf(_)
        | DR::DatatypeRestriction(..) => true,
        DR::DataComplementOf(x) => dr_has_set(x),
        _ => false,
    }
}

fn sorted_set<T: Ord>(mut v: Vec<T>) -> Vec<T> {
    v.sort();
    v.dedup();
    v
}

/// The canonical form: every nested set-valued operand list sorted and deduplicated.
#[must_use]
pub fn ce_canon<A: ForIRI>(c: &CE<A>) -> CE<A> {
    let b = |x: &CE<A>| Box::new(ce_canon(x));
    match c {
        CE::ObjectIntersectionOf(xs) => {
            CE::ObjectIntersectionOf(sorted_set(xs.iter().map(ce_canon).collect()))
        }
        CE::ObjectUnionOf(xs) => CE::ObjectUnionOf(sorted_set(xs.iter().map(ce_canon).collect())),
        CE::ObjectOneOf(xs) => CE::ObjectOneOf(sorted_set(xs.clone())),
        CE::ObjectComplementOf(x) => CE::ObjectComplementOf(b(x)),
        CE::ObjectSomeValuesFrom { ope, bce } => CE::ObjectSomeValuesFrom {
            ope: ope.clone(),
            bce: b(bce),
        },
        CE::ObjectAllValuesFrom { ope, bce } => CE::ObjectAllValuesFrom {
            ope: ope.clone(),
            bce: b(bce),
        },
        CE::ObjectMinCardinality { n, ope, bce } => CE::ObjectMinCardinality {
            n: *n,
            ope: ope.clone(),
            bce: b(bce),
        },
        CE::ObjectMaxCardinality { n, ope, bce } => CE::ObjectMaxCardinality {
            n: *n,
            ope: ope.clone(),
            bce: b(bce),
        },
        CE::ObjectExactCardinality { n, ope, bce } => CE::ObjectExactCardinality {
            n: *n,
            ope: ope.clone(),
            bce: b(bce),
        },
        CE::DataSomeValuesFrom { dp, dr } => CE::DataSomeValuesFrom {
            dp: dp.clone(),
            dr: dr_canon(dr),
        },
        CE::DataAllValuesFrom { dp, dr } => CE::DataAllValuesFrom {
            dp: dp.clone(),
            dr: dr_canon(dr),
        },
        CE::DataMinCardinality { n, dp, dr } => CE::DataMinCardinality {
            n: *n,
            dp: dp.clone(),
            dr: dr_canon(dr),
        },
        CE::DataMaxCardinality { n, dp, dr } => CE::DataMaxCardinality {
            n: *n,
            dp: dp.clone(),
            dr: dr_canon(dr),
        },
        CE::DataExactCardinality { n, dp, dr } => CE::DataExactCardinality {
            n: *n,
            dp: dp.clone(),
            dr: dr_canon(dr),
        },
        other => other.clone(),
    }
}

/// [`ce_canon`] for data ranges.
#[must_use]
pub fn dr_canon<A: ForIRI>(d: &DR<A>) -> DR<A> {
    match d {
        DR::DataIntersectionOf(xs) => {
            DR::DataIntersectionOf(sorted_set(xs.iter().map(dr_canon).collect()))
        }
        DR::DataUnionOf(xs) => DR::DataUnionOf(sorted_set(xs.iter().map(dr_canon).collect())),
        DR::DataComplementOf(x) => DR::DataComplementOf(Box::new(dr_canon(x))),
        DR::DataOneOf(xs) => DR::DataOneOf(sorted_set(xs.clone())),
        DR::DatatypeRestriction(dt, fs) => {
            DR::DatatypeRestriction(dt.clone(), sorted_set(fs.clone()))
        }
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::convert::convert_ontology;
    use horned_owl::io::ParserConfiguration;
    use horned_owl::io::ofn::reader::read;
    use horned_owl::model::RcStr;

    const HEAD: &str = "Prefix(:=<http://x/>)\nPrefix(owl:=<http://www.w3.org/2002/07/owl#>)\n\
        Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)\nOntology(<http://x/o>\n";

    fn onto(body: &str) -> SetOntology<RcStr> {
        let src = format!("{HEAD}{body}\n)\n");
        read(
            &mut std::io::Cursor::new(src),
            ParserConfiguration::default(),
        )
        .expect("parse")
        .0
    }

    /// The one n-ary component of `body`.
    fn nary(body: &str) -> C<RcStr> {
        let o = onto(body);
        let mut it = o.iter().filter(|ac| operand_count(&ac.component).is_some());
        let c = it.next().expect("an n-ary axiom").component.clone();
        assert!(it.next().is_none(), "one n-ary axiom per case");
        c
    }

    /// `canonical(lhs) == Some(rhs)` (or `None` when `rhs` is `None`). The parser refuses
    /// a one-operand axiom, so a `rhs` ending in the placeholder operand `:Zz`, `:zz` or
    /// `:zr` (individual, class, property) stands for `rhs` without it.
    fn canon_is(lhs: &str, rhs: Option<&str>) {
        let got = canonical(&nary(lhs));
        let want = rhs.map(|r| {
            let c = nary(r);
            if r.contains(":Zz") || r.contains(":zz") || r.contains(":zr") {
                drop_last(c)
            } else {
                c
            }
        });
        assert_eq!(got, want, "canonical({lhs})");
    }

    fn drop_last(c: C<RcStr>) -> C<RcStr> {
        let mut c = c;
        match &mut c {
            C::DifferentIndividuals(ax) => drop(ax.0.pop()),
            C::SameIndividual(ax) => drop(ax.0.pop()),
            C::DisjointClasses(ax) => drop(ax.0.pop()),
            C::EquivalentClasses(ax) => drop(ax.0.pop()),
            C::DisjointUnion(ax) => drop(ax.1.pop()),
            C::DisjointObjectProperties(ax) => drop(ax.0.pop()),
            C::EquivalentObjectProperties(ax) => drop(ax.0.pop()),
            C::DisjointDataProperties(ax) => drop(ax.0.pop()),
            C::EquivalentDataProperties(ax) => drop(ax.0.pop()),
            C::HasKey(ax) => drop(ax.vpe.pop()),
            _ => unreachable!(),
        }
        c
    }

    #[test]
    fn no_repeat_is_borrowed_for_every_kind() {
        for body in [
            "DifferentIndividuals(:a :b)",
            "SameIndividual(:a :b)",
            "DisjointClasses(:A :B)",
            "EquivalentClasses(:A :B)",
            "DisjointUnion(:D :A :B)",
            "DisjointObjectProperties(:r :s)",
            "EquivalentObjectProperties(:r :s)",
            "DisjointDataProperties(:p :q)",
            "EquivalentDataProperties(:p :q)",
            "HasKey(:A (:r :s) ())",
        ] {
            canon_is(body, None);
        }
    }

    #[test]
    fn a_repeat_collapses_for_every_kind() {
        for (lhs, rhs) in [
            (
                "DifferentIndividuals(:a :a :b)",
                "DifferentIndividuals(:a :b)",
            ),
            ("SameIndividual(:a :b :a)", "SameIndividual(:a :b)"),
            ("DisjointClasses(:A :A)", "DisjointClasses(:A :zz)"),
            ("EquivalentClasses(:A :B :A)", "EquivalentClasses(:A :B)"),
            ("DisjointUnion(:D :A :A)", "DisjointUnion(:D :A :zz)"),
            (
                "DisjointObjectProperties(ObjectInverseOf(:r) ObjectInverseOf(:r))",
                "DisjointObjectProperties(ObjectInverseOf(:r) :zr)",
            ),
            (
                "EquivalentObjectProperties(:r :r :s)",
                "EquivalentObjectProperties(:r :s)",
            ),
            (
                "DisjointDataProperties(:p :p)",
                "DisjointDataProperties(:p :zr)",
            ),
            (
                "EquivalentDataProperties(:q :p :q)",
                "EquivalentDataProperties(:q :p)",
            ),
            ("HasKey(:A (:r :r) (:p :p))", "HasKey(:A (:r) (:p :zr))"),
        ] {
            canon_is(lhs, Some(rhs));
        }
    }

    #[test]
    fn first_occurrences_are_kept_in_order() {
        canon_is(
            "DisjointClasses(:D :C :D :C :E :C)",
            Some("DisjointClasses(:D :C :E)"),
        );
        // A complex operand repeated exactly.
        canon_is(
            "DisjointClasses(ObjectSomeValuesFrom(:r :D) :E ObjectSomeValuesFrom(:r :D))",
            Some("DisjointClasses(ObjectSomeValuesFrom(:r :D) :E)"),
        );
    }

    /// One case per nested set-valued constructor: the two operands are structurally
    /// equal under the specification's set reading, though not as horned-owl lists.
    #[test]
    fn nested_set_operands_compare_as_sets() {
        for (x, y) in [
            ("ObjectIntersectionOf(:A :B)", "ObjectIntersectionOf(:B :A)"),
            (
                "ObjectIntersectionOf(:A :B)",
                "ObjectIntersectionOf(:A :B :A)",
            ),
            ("ObjectUnionOf(:A :B)", "ObjectUnionOf(:B :A)"),
            ("ObjectOneOf(:a :b)", "ObjectOneOf(:b :a :b)"),
            (
                "ObjectSomeValuesFrom(:r ObjectUnionOf(:A ObjectIntersectionOf(:B :C)))",
                "ObjectSomeValuesFrom(:r ObjectUnionOf(ObjectIntersectionOf(:C :B) :A))",
            ),
            (
                "ObjectMaxCardinality(2 :r ObjectIntersectionOf(:A :B))",
                "ObjectMaxCardinality(2 :r ObjectIntersectionOf(:B :A))",
            ),
            (
                "DataSomeValuesFrom(:p DataOneOf(\"1\"^^xsd:integer \"2\"^^xsd:integer))",
                "DataSomeValuesFrom(:p DataOneOf(\"2\"^^xsd:integer \"1\"^^xsd:integer))",
            ),
            (
                "DataSomeValuesFrom(:p DataUnionOf(xsd:integer xsd:string))",
                "DataSomeValuesFrom(:p DataUnionOf(xsd:string xsd:integer))",
            ),
            (
                "DataSomeValuesFrom(:p DataIntersectionOf(xsd:integer xsd:decimal))",
                "DataSomeValuesFrom(:p DataIntersectionOf(xsd:decimal xsd:integer))",
            ),
            (
                "DataSomeValuesFrom(:p DatatypeRestriction(xsd:integer xsd:minInclusive \
                 \"1\"^^xsd:integer xsd:maxInclusive \"5\"^^xsd:integer))",
                "DataSomeValuesFrom(:p DatatypeRestriction(xsd:integer xsd:maxInclusive \
                 \"5\"^^xsd:integer xsd:minInclusive \"1\"^^xsd:integer))",
            ),
            (
                "ObjectComplementOf(ObjectUnionOf(:A :B))",
                "ObjectComplementOf(ObjectUnionOf(:B :A))",
            ),
        ] {
            let lhs = format!("DisjointClasses({x} :E {y})");
            canon_is(&lhs, Some(&format!("DisjointClasses({x} :E)")));
        }
    }

    /// Operands that DENOTE the same thing but are not structurally equal stay apart:
    /// collapsing them would drop a real consequence (`A` unsat, `a ≠ a`).
    #[test]
    fn semantically_equal_operands_are_kept() {
        for body in [
            "DisjointClasses(:A ObjectComplementOf(ObjectComplementOf(:A)))",
            "DisjointClasses(:A ObjectIntersectionOf(:A owl:Thing))",
            "DisjointClasses(:A ObjectUnionOf(:A owl:Nothing))",
            "DisjointClasses(ObjectIntersectionOf(ObjectIntersectionOf(:A :B) :C) \
             ObjectIntersectionOf(:A ObjectIntersectionOf(:B :C)))",
            "DisjointClasses(owl:Thing ObjectIntersectionOf(owl:Thing owl:Thing))",
            "DisjointClasses(DataHasValue(:p \"1\"^^xsd:integer) \
             DataHasValue(:p \"01\"^^xsd:integer))",
            "DisjointObjectProperties(:r ObjectInverseOf(:r))",
            "DisjointClasses(ObjectMinCardinality(1 :r :A) ObjectMinCardinality(2 :r :A))",
        ] {
            canon_is(body, None);
        }
    }

    /// The hashed path (more than `PAIRWISE_MAX` operands) agrees with the pairwise one.
    #[test]
    fn long_operand_lists_dedup_by_canonical_key() {
        let named: Vec<String> = (0..20).map(|i| format!(":C{i}")).collect();
        let base = named.join(" ");
        canon_is(&format!("DisjointClasses({base})"), None);
        canon_is(
            &format!("DisjointClasses({base} :C3 :C0)"),
            Some(&format!("DisjointClasses({base})")),
        );
        canon_is(
            &format!("DisjointClasses(ObjectUnionOf(:A :B) {base} ObjectUnionOf(:B :A))"),
            Some(&format!("DisjointClasses(ObjectUnionOf(:A :B) {base})")),
        );
        canon_is(
            &format!("DisjointClasses({base} ObjectComplementOf(ObjectComplementOf(:C1)))"),
            None,
        );
    }

    /// `ce_eq` is equality of `ce_canon` forms, on every pair of a mixed list.
    #[test]
    fn ce_eq_is_canonical_form_equality() {
        let exprs = [
            ":A",
            ":B",
            "ObjectIntersectionOf(:A :B)",
            "ObjectIntersectionOf(:B :A :B)",
            "ObjectUnionOf(:A :B)",
            "ObjectSomeValuesFrom(:r ObjectIntersectionOf(:B :A))",
            "ObjectSomeValuesFrom(:r ObjectIntersectionOf(:A :B))",
            "ObjectAllValuesFrom(:r ObjectIntersectionOf(:A :B))",
            "ObjectOneOf(:a :b)",
            "ObjectOneOf(:b :a)",
            "ObjectComplementOf(ObjectComplementOf(:A))",
        ];
        let ces: Vec<CE<RcStr>> = exprs
            .iter()
            .map(|e| match nary(&format!("EquivalentClasses({e} :Z)")) {
                C::EquivalentClasses(ax) => ax.0[0].clone(),
                _ => unreachable!(),
            })
            .collect();
        for a in &ces {
            for b in &ces {
                assert_eq!(ce_eq(a, b), ce_canon(a) == ce_canon(b), "{a:?} vs {b:?}");
            }
        }
    }

    #[test]
    fn count_separates_vacuous_axioms() {
        let o = onto(
            "DisjointClasses(:A :A) DifferentIndividuals(:a :a :b) DisjointUnion(:D :C :C) \
             DisjointClasses(:A :B)",
        );
        assert_eq!(
            count(&o),
            CollapsedOperands {
                axioms: 3,
                vacuous: 1
            }
        );
        assert_eq!(convert_ontology(&o).unwrap().collapsed_operands, count(&o));
        assert!(count(&onto("DisjointClasses(:A :B)")).is_empty());
    }

    /// Repeating operands changes nothing in the converted ontology: the same axioms, in
    /// the same order, over the same vocabulary and concepts.
    #[test]
    fn repeated_operands_convert_to_the_same_ontology() {
        let decl = "Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C)) \
                    Declaration(Class(:D)) Declaration(ObjectProperty(:r)) \
                    Declaration(ObjectProperty(:s)) Declaration(NamedIndividual(:a)) \
                    Declaration(NamedIndividual(:b))";
        for (plain, repeated) in [
            ("DisjointClasses(:A :B)", "DisjointClasses(:A :B :A)"),
            // The canonical `DisjointClasses(:B :A)` is its own component, as it would be
            // written without the repeat; one equal to another component merges with it.
            (
                "DisjointClasses(:B :A) DisjointClasses(:A :B)",
                "DisjointClasses(:B :A :B) DisjointClasses(:A :B)",
            ),
            (
                "DisjointClasses(:A :B)",
                "DisjointClasses(:A :B :B) DisjointClasses(:A :B)",
            ),
            (
                "DifferentIndividuals(:a :b)",
                "DifferentIndividuals(:a :a :b)",
            ),
            ("SameIndividual(:a :b)", "SameIndividual(:a :b :b)"),
            ("DisjointUnion(:D :A :B)", "DisjointUnion(:D :A :B :A)"),
            (
                "DisjointObjectProperties(:r :s)",
                "DisjointObjectProperties(:r :s :r)",
            ),
            (
                "EquivalentClasses(:C ObjectIntersectionOf(:A :B))",
                "EquivalentClasses(:C ObjectIntersectionOf(:A :B) ObjectIntersectionOf(:B :A))",
            ),
        ] {
            let a = convert_ontology(&onto(&format!("{decl} {plain} SubClassOf(:C :D)"))).unwrap();
            let b =
                convert_ontology(&onto(&format!("{decl} {repeated} SubClassOf(:C :D)"))).unwrap();
            assert_eq!(
                format!("{:?}", a.axioms),
                format!("{:?}", b.axioms),
                "{repeated}"
            );
            let ids = |o: &crate::InternalOntology| {
                let v = &o.vocabulary;
                let c: Vec<String> = v.classes().map(|(i, s)| format!("{i:?}={s}")).collect();
                let r: Vec<String> = v.roles().map(|(i, s)| format!("{i:?}={s}")).collect();
                let n: Vec<String> = v.individuals().map(|(i, s)| format!("{i:?}={s}")).collect();
                (c, r, n)
            };
            assert_eq!(ids(&a), ids(&b), "{repeated}");
            // The pool's id table, in id order (its reverse map is a `HashMap`).
            let pool = |o: &crate::InternalOntology| {
                let d = format!("{:?}", o.concepts);
                d[..d.find(", by_expr").expect("ConceptPool's Debug shape")].to_string()
            };
            assert_eq!(pool(&a), pool(&b), "{repeated}");
            assert_eq!(a.dropped, b.dropped);
        }
    }

    /// A one-operand axiom says nothing and is not in the IR, but its entities still
    /// are; a one-member `DisjointUnion` is `D ≡ C`.
    #[test]
    fn vacuous_axioms_keep_their_entities() {
        use crate::ontology::Axiom;
        let o = convert_ontology(&onto(
            "DisjointClasses(:OnlyHere :OnlyHere) DifferentIndividuals(:solo :solo) \
             DisjointObjectProperties(:lonely :lonely)",
        ))
        .unwrap();
        assert!(o.axioms.iter().all(|a| !matches!(
            a,
            Axiom::DisjointClasses(_)
                | Axiom::DifferentIndividuals(_)
                | Axiom::DisjointObjectProperties(_)
        )));
        assert!(o.vocabulary.class_id("http://x/OnlyHere").is_some());
        assert!(o.vocabulary.individual_id("http://x/solo").is_some());
        assert!(o.vocabulary.role_id("http://x/lonely").is_some());
        assert!(o.dropped.is_empty());
        let du = convert_ontology(&onto("DisjointUnion(:D :C :C)")).unwrap();
        assert!(
            du.axioms
                .iter()
                .any(|a| matches!(a, Axiom::EquivalentClasses(v) if v.len() == 2)),
            "{:?}",
            du.axioms
        );
        assert!(
            !du.axioms
                .iter()
                .any(|a| matches!(a, Axiom::DisjointUnion { .. }))
        );
    }
}
