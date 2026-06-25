//! Seed-saturation query API for the ⊔ failed-literal look-ahead gate.
//!
//! `build_base` runs the marker saturator once; `seed_unsat` answers
//! "is `⊓seed` unsatisfiable?" by cloning the base engine, injecting the
//! seed into a reserved synthetic class `X`, running to fixpoint, and
//! reading `X ⊑ ⊥`. The base is immutable across calls (clone-per-call) —
//! naive but correct; the gate measures branch counts, not wall.

use owl_dl_core::{ClassId, ConceptExpr, ConceptId, InternalOntology, RoleId};

use crate::{WorklistEngine, build_run_engine_with_reserved};

/// Once-built, fully-run base engine for seed-saturation queries.
///
/// Build with [`build_base`]; then call [`SeedSaturator::seed_unsat`] any number
/// of times, each of which clones the base, injects a small seed set for the
/// reserved synthetic class `X`, runs to fixpoint, and returns whether `X ⊑ ⊥`.
pub struct SeedSaturator {
    base: WorklistEngine,
    reserved_x: ClassId,
}

/// Build and fully run the base saturation engine for the given ontology,
/// reserving one extra synthetic class id `X` for seed-unsat queries.
///
/// The returned [`SeedSaturator`] holds the frozen base state; individual
/// `seed_unsat` calls clone that base without mutating it.
#[must_use]
pub fn build_base(internal: &InternalOntology) -> SeedSaturator {
    let (base, reserved_x) = build_run_engine_with_reserved(internal);
    SeedSaturator { base, reserved_x }
}

impl SeedSaturator {
    /// True iff `⊓atomic_seed ⊓ ⊓∃exists_seed` is unsatisfiable per the
    /// marker saturator.
    ///
    /// Clones the base engine per call.  For each `aᵢ` in `atomic_seed`
    /// injects `X ⊑ aᵢ`; for each `(rⱼ, cⱼ)` in `exists_seed` injects
    /// `X ⊑ ∃rⱼ.cⱼ`.  Then runs to fixpoint and reads `X ⊑ ⊥`.
    #[must_use]
    pub fn seed_unsat(&self, atomic_seed: &[ClassId], exists_seed: &[(RoleId, ClassId)]) -> bool {
        let mut e = self.base.clone();
        let x = self.reserved_x;
        for &a in atomic_seed {
            e.inject_subsumer(x, a);
        }
        for &(r, c) in exists_seed {
            e.inject_existential(x, r, c);
        }
        e.run();
        e.is_unsat_class(x)
    }

    /// Returns the atomic [`ClassId`] for an `Atomic` concept, or `None`
    /// for any compound concept.  Used by the look-ahead gate (Unit 2) to
    /// build the atomic seed subset from a label set of `ConceptId`s.
    #[must_use]
    pub fn class_of_concept(&self, internal: &InternalOntology, cid: ConceptId) -> Option<ClassId> {
        match internal.concepts.get(cid) {
            ConceptExpr::Atomic(id) => Some(*id),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use horned_owl::io::ParserConfiguration;
    use horned_owl::io::ofn::reader::read;
    use horned_owl::model::RcStr;
    use horned_owl::ontology::set::SetOntology;
    use owl_dl_core::InternalOntology;
    use owl_dl_core::convert::convert_ontology;
    use std::io::Cursor;

    // -----------------------------------------------------------------------
    // Parse helpers (mirror the lib.rs test module)
    // -----------------------------------------------------------------------

    fn parse_internal(src: &str) -> InternalOntology {
        let mut reader = Cursor::new(src);
        let (onto, _): (SetOntology<RcStr>, _) =
            read(&mut reader, ParserConfiguration::default()).expect("ofn parses");
        convert_ontology(&onto).expect("conversion")
    }

    const HEADER: &str = "\
Prefix(:=<http://rustdl.test/>)\n\
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)\n";

    fn class_id(internal: &InternalOntology, local: &str) -> ClassId {
        internal
            .vocabulary
            .class_id(&format!("http://rustdl.test/{local}"))
            .expect("class declared")
    }

    fn role_id(internal: &InternalOntology, local: &str) -> owl_dl_core::ir::RoleId {
        internal
            .vocabulary
            .role_id(&format!("http://rustdl.test/{local}"))
            .expect("role declared")
    }

    // -----------------------------------------------------------------------
    // Fixture helpers
    // -----------------------------------------------------------------------

    struct DisjointAbIds {
        a: ClassId,
        b: ClassId,
        c: ClassId,
    }

    /// Tiny ontology: `DisjointClasses(A, B)`; C is unrelated.
    fn build_disjoint_ab() -> (InternalOntology, DisjointAbIds) {
        let internal = parse_internal(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:A))\n\
    Declaration(Class(:B))\n\
    Declaration(Class(:C))\n\
    DisjointClasses(:A :B)\n\
)\n"
        ));
        let ids = DisjointAbIds {
            a: class_id(&internal, "A"),
            b: class_id(&internal, "B"),
            c: class_id(&internal, "C"),
        };
        (internal, ids)
    }

    struct ForallKeyIds {
        d: ClassId,
        r: owl_dl_core::ir::RoleId,
        /// `ClassId` for class A — disjoint with K, out-of-range.
        a_class: ClassId,
        /// `ClassId` for class K — the in-range filler class.
        k_member: ClassId,
    }

    /// Minimal `ForallKey` fixture: D ⊑ ∃r.K, D ⊑ ∀r.K, r functional,
    /// DisjointClasses(K, A).
    ///
    /// Pattern: the horn fixpoint MISSES `D ⊓ ∃r.A → ⊥` (because A ∉ K and
    /// r is functional) but the seed-sat should detect it via the
    /// functional-merge path: X ⊑ D inherits ∃r.K; inject X ⊑ ∃r.A triggers
    /// functional merge → synthetic Γ ≡ K ⊓ A; DisjointClasses(K,A) → Γ ⊑ ⊥
    /// → X ⊑ ∃r.Γ → X ⊑ ⊥.
    fn build_forall_key_clash() -> (InternalOntology, ForallKeyIds) {
        let internal = parse_internal(&format!(
            "{HEADER}\
Ontology(<http://rustdl.test/test>\n\
    Declaration(Class(:D))\n\
    Declaration(Class(:K))\n\
    Declaration(Class(:A))\n\
    Declaration(ObjectProperty(:r))\n\
    FunctionalObjectProperty(:r)\n\
    DisjointClasses(:K :A)\n\
    SubClassOf(:D ObjectSomeValuesFrom(:r :K))\n\
    SubClassOf(:D ObjectAllValuesFrom(:r :K))\n\
)\n"
        ));
        let ids = ForallKeyIds {
            d: class_id(&internal, "D"),
            r: role_id(&internal, "r"),
            a_class: class_id(&internal, "A"),
            k_member: class_id(&internal, "K"),
        };
        (internal, ids)
    }

    // -----------------------------------------------------------------------
    // Tests
    // -----------------------------------------------------------------------

    /// Told-disjoint seed: A and B are disjoint → seeding {A, B} is unsat;
    /// A alone or {A, C} (no disjointness) are satisfiable.
    #[test]
    fn told_disjoint_seed_is_unsat() {
        let (internal, ids) = build_disjoint_ab();
        let sat = build_base(&internal);

        // Positive: A and B disjoint → X ⊑ A ⊓ B → X ⊑ ⊥
        assert!(
            sat.seed_unsat(&[ids.a, ids.b], &[]),
            "A ⊓ B must be unsat (told DisjointClasses)"
        );

        // Negative: A alone is satisfiable
        assert!(
            !sat.seed_unsat(&[ids.a], &[]),
            "A alone must be satisfiable"
        );

        // Negative: A and C have no disjointness declared → compatible
        assert!(
            !sat.seed_unsat(&[ids.a, ids.c], &[]),
            "A ⊓ C must be satisfiable (no disjointness)"
        );
    }

    /// ForallKey-driven clash: D ⊑ ∃r.K with r functional; K ⊓ A = ⊥.
    /// Seeding D + ∃r.A triggers functional merge → Tseitin Γ ≡ K ⊓ A → ⊥.
    /// Seeding D + ∃r.K (same filler D already carries) is satisfiable.
    ///
    /// This reproduces the pattern the horn fixpoint misses: `∀r.K + ∃r.A`
    /// with A ∉ K is unsatisfiable, but the horn fixpoint (EL without the
    /// functional-merge rule) cannot detect it.  The seed-sat path exercises
    /// the Phase-2a functional-merge rule on the cloned engine.
    #[test]
    fn forall_key_seed_is_unsat() {
        let (internal, ids) = build_forall_key_clash();
        let sat = build_base(&internal);

        // Verify the base alone does not flag D as unsat (sanity check).
        assert!(
            !sat.seed_unsat(&[ids.d], &[]),
            "D alone must be satisfiable in the base"
        );

        // Positive: D (carries ∃r.K) + ∃r.A (disjoint with K) → unsat via
        // functional merge: two r-witnesses K and A collapse to Γ ≡ K⊓A ⊑ ⊥.
        assert!(
            sat.seed_unsat(&[ids.d], &[(ids.r, ids.a_class)]),
            "D ⊓ ∃r.A must be unsat (functional merge K⊓A clashes)"
        );

        // Negative: D + ∃r.K — K is the filler D already asserts; the
        // dedup in push_fact prevents any merge, so X is satisfiable.
        assert!(
            !sat.seed_unsat(&[ids.d], &[(ids.r, ids.k_member)]),
            "D ⊓ ∃r.K must be satisfiable (same filler as D's told fact)"
        );
    }
}
