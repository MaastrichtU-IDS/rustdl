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
/// Returns the `ToldTables` it built **iff `onto` was left unmodified**, so the
/// next pass can reuse them instead of rebuilding.
///
/// `build_told_tables` is not cheap on a large `TBox`: its axiom scan is O(#axioms)
/// with a pool lookup per axiom, measured at **3.9 s per call** on `ore_ont_3524`
/// (2,097,631 axioms), and `convert_ontology` called it here and again in
/// `derive_forced_disjuncts` immediately afterwards. Where this pass appends
/// nothing, the second build is provably redundant — verified by fingerprinting
/// the closure: both calls produced identical tables (`ore_ont_3524`
/// `1cae6c7a85864700`, `ore_ont_9674` `e30156ebb2b0bd21`).
///
/// The `Some` arm is gated on `onto.axioms.len()` being unchanged, which is the
/// robust detector: EVERY mutation in this function sits inside a loop that
/// pushes an axiom, so "no axiom appended" is equivalent to "not mutated". A
/// concept interned without an axiom push could not affect the tables anyway —
/// interning does not add to `vocabulary`, which is what sizes them.
pub fn derive_disjunction_existentials(onto: &mut InternalOntology) -> Option<ToldTables> {
    let axioms_before = onto.axioms.len();
    let told = build_told_tables(onto);
    // Phase 1 (immutable borrow): collect (sub, role, common-class) for
    // `∃R.(union)` supers, and (sub, common-class) for **bare** `(union)`
    // supers (e.g. the disjunctive-data-property-domain GCI).
    let mut triples: Vec<(ConceptId, Role, ClassId)> = Vec::new();
    let mut bare: Vec<(ConceptId, ClassId)> = Vec::new();
    // Disjunctive object-property domain/range: `domain(R) = D₁ ⊔ … ⊔ Dₙ` with all
    // disjuncts sharing a common told-subsumer C ⟹ `domain(R) ⊑ C`, so
    // `ObjectPropertyDomain(R, C)` is entailed (sound, weaker). The saturator's
    // Pass-1 domain/range handler only registers ATOMIC domains, so it drops the
    // disjunctive form entirely — costing the whole `∃R / ∃R.Self → domain` chain
    // (olia ore_ont_4827: domain(hasCase) = Adjective ⊔ Article ⊔ Noun ⊔ Numeral ⊔
    // PronounOrDeterminer, all ⊑ MorphosyntacticCategory transitively). `(role, C)`.
    let mut dom_ranges: Vec<(Role, ClassId, bool)> = Vec::new(); // (role, C, is_range)
    for ax in &onto.axioms {
        match ax {
            Axiom::SubClassOf { sub, sup } => {
                collect_from_sup(*sub, *sup, &onto.concepts, &told, &mut triples, &mut bare);
            }
            Axiom::ObjectPropertyDomain { role, domain }
                if matches!(onto.concepts.get(*domain), ConceptExpr::Or(_)) =>
            {
                for c in minimal_common_subsumers(*domain, &onto.concepts, &told) {
                    dom_ranges.push((*role, c, false));
                }
            }
            Axiom::ObjectPropertyRange { role, range }
                if matches!(onto.concepts.get(*range), ConceptExpr::Or(_)) =>
            {
                for c in minimal_common_subsumers(*range, &onto.concepts, &told) {
                    dom_ranges.push((*role, c, true));
                }
            }
            // SP-B2c: union class `X ≡ D₁⊔…⊔Dₙ` (EquivalentClasses with an atomic
            // member `X` and an `Or` member). Two sound inferences:
            //   #1 common-subsumer (`X ⊑ ⊔Dᵢ` direction): `X ⊑ E` for each common
            //      told-subsumer `E` of the disjuncts.
            //   #2 disjunct⊑union (`⊔Dᵢ ⊑ X` direction, EQUIVALENCE-ONLY): `Dᵢ ⊑ X`
            //      for each atomic disjunct (`Dᵢ ⊑ ⊔Dⱼ ≡ X`).
            // Both fed via `bare` (`SubClassOf(sub, atomic(c))`). `disjunction_
            // existential` otherwise only sees `SubClassOf`-Or, missing both here.
            Axiom::EquivalentClasses(members) => {
                for i in 0..members.len() {
                    for j in 0..members.len() {
                        if i == j {
                            continue;
                        }
                        if let (ConceptExpr::Atomic(x), ConceptExpr::Or(disjuncts)) =
                            (onto.concepts.get(members[i]), onto.concepts.get(members[j]))
                        {
                            let x = *x;
                            for e in minimal_common_subsumers(members[j], &onto.concepts, &told) {
                                bare.push((members[i], e));
                            }
                            for &d in disjuncts {
                                if matches!(onto.concepts.get(d), ConceptExpr::Atomic(_)) {
                                    bare.push((d, x));
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
    if triples.is_empty() && bare.is_empty() && dom_ranges.is_empty() {
        // Nothing to append, so `told` is still valid for the caller's next pass.
        // This is the common case and the whole point of the return value.
        return Some(told);
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
    // Bare common-subsumer subsumptions `X ⊑ E` (E atomic). Feeds the
    // saturator directly — no ∃ wrapper, no tableau case-split needed.
    for (sub, c) in bare {
        let sup = onto.concepts.atomic(c);
        if sub == sup {
            continue;
        }
        onto.axioms.push(Axiom::SubClassOf { sub, sup });
    }
    // Derived atomic domain/range from a disjunctive one (sound under-approx).
    for (role, c, is_range) in dom_ranges {
        let cls = onto.concepts.atomic(c);
        if is_range {
            onto.axioms
                .push(Axiom::ObjectPropertyRange { role, range: cls });
        } else {
            onto.axioms
                .push(Axiom::ObjectPropertyDomain { role, domain: cls });
        }
    }

    if onto.axioms.len() == axioms_before {
        Some(told)
    } else {
        None
    }
}

/// Handle a single `SubClassOf` super-concept: a direct `∃R.(union)`, a
/// bare `(union)`, or each such conjunct of a top-level `And`.
/// `∃R.(union)` supers append `(sub, R, C)` to `out`; bare `(union)`
/// supers append `(sub, C)` to `bare` (C the common told-subsumer).
fn collect_from_sup(
    sub: ConceptId,
    sup: ConceptId,
    pool: &ConceptPool,
    told: &ToldTables,
    out: &mut Vec<(ConceptId, Role, ClassId)>,
    bare: &mut Vec<(ConceptId, ClassId)>,
) {
    match pool.get(sup) {
        ConceptExpr::Some(role, body) => {
            for c in minimal_common_subsumers(*body, pool, told) {
                out.push((sub, *role, c));
            }
        }
        // Bare disjunctive super `X ⊑ (D₁ ⊔ … ⊔ Dₙ)`: `(⊔Dᵢ) ⊑ C` for
        // every common told-subsumer C, hence `X ⊑ C`. `sup` itself is
        // the `Or` body that `minimal_common_subsumers` expects.
        ConceptExpr::Or(_) => {
            for c in minimal_common_subsumers(sup, pool, told) {
                bare.push((sub, c));
            }
        }
        ConceptExpr::And(operands) => {
            for &op in operands {
                match pool.get(op) {
                    ConceptExpr::Some(role, body) => {
                        for c in minimal_common_subsumers(*body, pool, told) {
                            out.push((sub, *role, c));
                        }
                    }
                    ConceptExpr::Or(_) => {
                        for c in minimal_common_subsumers(op, pool, told) {
                            bare.push((sub, c));
                        }
                    }
                    _ => {}
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
    // STRICTLY told-below `C` (the saturator recovers the weaker supers from the
    // minimal ones, so emitting the whole chain is redundant). "Strictly" — the
    // `&& !is_told_sub(c, other)` guard — is load-bearing: without it, two
    // mutually-told-sub (i.e. told-EQUIVALENT) common subsumers each count as
    // "below" the other, so BOTH are dropped and the whole set can collapse to
    // empty (olia ore_ont_4827: `MorphosyntacticCategory ≡ Word`, both common
    // subsumers of every disjunct, were dropped — losing the domain entirely).
    common
        .iter()
        .copied()
        .filter(|&c| {
            !common.iter().any(|&other| {
                other != c && told.is_told_sub(other, c) && !told.is_told_sub(c, other)
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::ir::{ClassId, ConceptExpr, Role};
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

    /// Whether `onto` has the bare atomic subsumption `sub ⊑ sup`.
    fn has_atomic_sub(
        onto: &crate::ontology::InternalOntology,
        sub: ClassId,
        sup: ClassId,
    ) -> bool {
        onto.axioms.iter().any(|ax| {
            matches!(ax, crate::ontology::Axiom::SubClassOf { sub: s, sup: p }
                if matches!(onto.concepts.get(*s), ConceptExpr::Atomic(c) if *c == sub)
                    && matches!(onto.concepts.get(*p), ConceptExpr::Atomic(c) if *c == sup))
        })
    }

    /// The SAO/BFO pattern end-to-end: `C ⊑ DataHasValue(p, "v")` +
    /// `DataPropertyDomain(p, D1 ⊔ D2)` with `D1,D2 ⊑ E` must yield the
    /// bare subsumption `C ⊑ E` (common told-subsumer of the disjunctive
    /// domain), via `data_axioms` → the convert-time union GCI → this
    /// pass. Mirrors `sao1785599611 ⊑ snap#Continuant`.
    #[test]
    fn disjunctive_data_domain_yields_common_subsumer() {
        let src = "\
Prefix(:=<http://t.org/#>)
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)
Ontology(
  Declaration(Class(:C)) Declaration(Class(:D1)) Declaration(Class(:D2)) Declaration(Class(:E))
  Declaration(DataProperty(:p))
  SubClassOf(:C DataHasValue(:p \"v\"^^xsd:string))
  DataPropertyDomain(:p ObjectUnionOf(:D1 :D2))
  SubClassOf(:D1 :E) SubClassOf(:D2 :E)
)
";
        let (set_onto, _): (SetOntology<RcStr>, _) =
            read(&mut Cursor::new(src), ParserConfiguration::default()).expect("parses");
        let onto = crate::convert::convert_ontology(&set_onto).expect("converts");
        let cid = |iri: &str| onto.vocabulary.class_id(iri).expect("declared");
        assert!(
            has_atomic_sub(&onto, cid("http://t.org/#C"), cid("http://t.org/#E")),
            "expected C ⊑ E from the disjunctive data-property domain"
        );
    }

    /// Negative: a disjunctive domain whose members share NO common told-
    /// subsumer must emit no bare subsumption — soundness floor (no FP).
    #[test]
    fn disjunctive_data_domain_no_common_subsumer_is_silent() {
        let src = "\
Prefix(:=<http://t.org/#>)
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)
Ontology(
  Declaration(Class(:C)) Declaration(Class(:D1)) Declaration(Class(:D2))
  Declaration(DataProperty(:p))
  SubClassOf(:C DataHasValue(:p \"v\"^^xsd:string))
  DataPropertyDomain(:p ObjectUnionOf(:D1 :D2))
)
";
        let (set_onto, _): (SetOntology<RcStr>, _) =
            read(&mut Cursor::new(src), ParserConfiguration::default()).expect("parses");
        let onto = crate::convert::convert_ontology(&set_onto).expect("converts");
        let cid = |iri: &str| onto.vocabulary.class_id(iri).expect("declared");
        let c = cid("http://t.org/#C");
        // C must not gain a spurious atomic super (D1/D2 share none).
        assert!(
            !has_atomic_sub(&onto, c, cid("http://t.org/#D1"))
                && !has_atomic_sub(&onto, c, cid("http://t.org/#D2")),
            "no common subsumer ⇒ no bare subsumption"
        );
    }

    /// Soundness gate: a domain union with a NON-atomic member must be
    /// rejected wholesale — the told tables can't see the non-atomic
    /// member, so a common-subsumer over the atomic subset would be
    /// unsound. Here `C ⊑ E` would NOT actually be entailed (the third
    /// domain disjunct `∃R.⊤` is not `⊑ E`), so we must emit nothing.
    #[test]
    fn disjunctive_data_domain_with_nonatomic_member_emits_nothing() {
        let src = "\
Prefix(:=<http://t.org/#>)
Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)
Ontology(
  Declaration(Class(:C)) Declaration(Class(:D1)) Declaration(Class(:D2)) Declaration(Class(:E)) Declaration(Class(:Z))
  Declaration(DataProperty(:p)) Declaration(ObjectProperty(:R))
  SubClassOf(:C DataHasValue(:p \"v\"^^xsd:string))
  DataPropertyDomain(:p ObjectUnionOf(:D1 :D2 ObjectSomeValuesFrom(:R :Z)))
  SubClassOf(:D1 :E) SubClassOf(:D2 :E)
)
";
        let (set_onto, _): (SetOntology<RcStr>, _) =
            read(&mut Cursor::new(src), ParserConfiguration::default()).expect("parses");
        let onto = crate::convert::convert_ontology(&set_onto).expect("converts");
        let cid = |iri: &str| onto.vocabulary.class_id(iri).expect("declared");
        assert!(
            !has_atomic_sub(&onto, cid("http://t.org/#C"), cid("http://t.org/#E")),
            "non-atomic domain disjunct ⇒ no inference (unsound otherwise)"
        );
    }
}
